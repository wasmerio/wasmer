//! A regular file's contents: written blocks, and the ranges of blocks allocated to it.
//!
//! Allocation is kept as ranges, so `fallocate`, hole punching and truncation cost the number of
//! ranges and stored blocks they touch, never the length of the byte range: allocating a
//! terabyte is one range. Blocks `fallocate` allocated hold no memory until they are written,
//! and read as zeroes; like tmpfs's not-yet-uptodate pages they count as space used but are
//! holes to `SEEK_DATA`/`SEEK_HOLE`.
//!
//! The bytes live in one of two stores. The heap keeps the written blocks themselves. A
//! filesystem made with a [`Provider`] keeps each file in a shared memory object instead, made
//! when the file first holds data, so that `MAP_SHARED` mappings show the file's own pages, as
//! tmpfs keeps every file in shmem (`mm/shmem.c`). The object then knows which pages hold data
//! (`SEEK_DATA`/`SEEK_HOLE` ask it), and its length is a high-water mark of the file's size and
//! of every mapping's end: it never shrinks, so no mapping reaches past it on the host, and
//! truncation frees the pages past the new size instead (`shmem_truncate_range`). Writes
//! through a mapping are not charged to the filesystem's space, as there is no page fault to
//! charge them at.

use std::{collections::BTreeMap, io, sync::Arc};

use wasixv2_abi::Errno;
use wasixv2_pages::{Provider, Seek, SharedPages};
use wasixv2_vfs::{FsResult, SeekHole};

use crate::{BLOCK, MAX_SIZE, Space};

/// The contents of a regular file. Every block holding data is in `alloc`.
pub(crate) struct Data {
    pub(crate) size: u64,
    store: Store,
    /// Blocks charged to the filesystem's space: written ones and those `fallocate` reserved.
    alloc: Ranges,
    /// Makes the shared memory object the file moves to when it first holds data.
    provider: Option<Arc<dyn Provider>>,
}

enum Store {
    /// Blocks holding data, by index.
    Heap(BTreeMap<u64, Box<[u8]>>),
    /// A shared memory object holding the whole file, which mappings of it show.
    Shared(Arc<dyn SharedPages>),
}

/// The errno for a shared memory object the host failed to read, write or resize: like a page
/// shmem could not allocate.
fn host_errno(_: io::Error) -> Errno {
    Errno::ENOMEM
}

/// Makes `pages` at least `end` bytes long; it never shrinks.
fn reach(pages: &dyn SharedPages, end: u64) -> FsResult<()> {
    if pages.size() < end {
        pages.resize(end).map_err(host_errno)?;
    }
    Ok(())
}

impl Data {
    /// An empty file, kept in shared memory objects from `provider` if there is one.
    pub(crate) fn new(provider: Option<Arc<dyn Provider>>) -> Self {
        Self {
            size: 0,
            store: Store::Heap(BTreeMap::new()),
            alloc: Ranges::default(),
            provider,
        }
    }

    /// Blocks charged to this file.
    pub(crate) fn allocated(&self) -> u64 {
        self.alloc.total
    }

    /// Blocks of memory the file holds, for `st_blocks`: those charged, or those the shared
    /// object holds if more, as pages written through a mapping are never charged.
    pub(crate) fn blocks(&self) -> u64 {
        match &self.store {
            Store::Heap(_) => self.alloc.total,
            Store::Shared(pages) => self.alloc.total.max(pages.allocated().div_ceil(BLOCK)),
        }
    }

    /// Moves the file into a shared memory object while it holds no data, so nothing needs
    /// copying. Where the provider fails, the file stays on the heap, and once it holds data
    /// there it stays for good: its mappings are copies then.
    fn promote(&mut self) {
        let Some(provider) = &self.provider else {
            return;
        };
        if let Store::Heap(blocks) = &self.store
            && blocks.is_empty()
            && let Ok(pages) = provider.shared_pages(0)
        {
            self.store = Store::Shared(pages);
            self.provider = None;
        }
    }

    /// The shared memory object holding the file, at least `end` bytes long, if it has one.
    pub(crate) fn shared_pages(&mut self, end: u64) -> FsResult<Option<Arc<dyn SharedPages>>> {
        self.promote();
        let Store::Shared(pages) = &self.store else {
            return Ok(None);
        };
        reach(&**pages, end)?;
        Ok(Some(pages.clone()))
    }

    pub(crate) fn read(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let end = self.size.min(offset.saturating_add(buf.len() as u64));
        let done = end.saturating_sub(offset) as usize;
        let blocks = match &self.store {
            Store::Heap(blocks) => blocks,
            Store::Shared(pages) => {
                pages
                    .read_at(&mut buf[..done], offset)
                    .map_err(host_errno)?;
                return Ok(done);
            }
        };
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let out = &mut buf[(pos - offset) as usize..][..len];
            match blocks.get(&index) {
                Some(page) => out.copy_from_slice(&page[start..start + len]),
                None => out.fill(0),
            }
            pos += len as u64;
        }
        Ok(done)
    }

    /// Writes `data` at `offset`, charging every block it allocates to `space`. Stops at the
    /// first block the space cannot hold: a short count, or `ENOSPC` if nothing was written
    /// (`generic_perform_write`).
    pub(crate) fn write(&mut self, space: &Space, data: &[u8], offset: u64) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|&end| end <= MAX_SIZE)
            .ok_or(Errno::EFBIG)?;
        self.promote();
        let reached = match &self.store {
            Store::Heap(_) => self.write_blocks(space, data, offset, end),
            Store::Shared(_) => self.charge(space, offset, end),
        };
        if reached == offset {
            return Err(Errno::ENOSPC);
        }
        if let Store::Shared(pages) = &self.store {
            let pages = pages.clone();
            self.extend(reached)?;
            reach(&*pages, reached)?;
            let data = &data[..(reached - offset) as usize];
            pages.write_at(data, offset).map_err(host_errno)?;
        }
        self.size = self.size.max(reached);
        Ok((reached - offset) as usize)
    }

    /// Writes `data` into heap blocks up to `end`, as far as the space holds them; returns
    /// where it stopped.
    fn write_blocks(&mut self, space: &Space, data: &[u8], offset: u64, end: u64) -> u64 {
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let Some(page) = self.page(space, index) else {
                break;
            };
            page[start..start + len].copy_from_slice(&data[(pos - offset) as usize..][..len]);
            pos += len as u64;
        }
        pos
    }

    /// Charges the blocks under `offset..end` not charged yet, as far as the space holds them;
    /// returns where it stopped.
    fn charge(&mut self, space: &Space, offset: u64, end: u64) -> u64 {
        for index in offset / BLOCK..end.div_ceil(BLOCK) {
            if !self.alloc.contains(index) {
                if space.charge_blocks(1).is_err() {
                    return (index * BLOCK).max(offset);
                }
                self.alloc.insert(index, index + 1);
            }
        }
        end
    }

    /// The heap block `index`, allocated and charged if it holds no data yet; `None` if the
    /// space is full.
    fn page(&mut self, space: &Space, index: u64) -> Option<&mut Box<[u8]>> {
        let Store::Heap(blocks) = &mut self.store else {
            return None;
        };
        if !blocks.contains_key(&index) {
            if !self.alloc.contains(index) {
                space.charge_blocks(1).ok()?;
                self.alloc.insert(index, index + 1);
            }
            blocks.insert(index, vec![0; BLOCK as usize].into_boxed_slice());
        }
        blocks.get_mut(&index)
    }

    /// The size grows to `size`: in a shared object, whole pages past the old end that a
    /// mapping wrote (where Linux would have raised `SIGBUS`) read zero again, as every byte a
    /// file grows by must.
    fn extend(&mut self, size: u64) -> FsResult<()> {
        let Store::Shared(pages) = &self.store else {
            return Ok(());
        };
        let from = self.size.div_ceil(BLOCK) * BLOCK;
        let to = size.min(pages.size());
        if from < to {
            pages.punch_hole(from, to - from).map_err(host_errno)?;
        }
        Ok(())
    }

    /// Sets the size. Shrinking frees every block past the new end, including blocks allocated
    /// past the old end with `FALLOC_FL_KEEP_SIZE` (`shmem_setattr`); growing leaves a hole.
    pub(crate) fn truncate(&mut self, space: &Space, size: u64) -> FsResult<()> {
        if size >= self.size {
            self.extend(size)?;
            self.size = size;
            return Ok(());
        }
        let first_gone = size.div_ceil(BLOCK);
        space.release_blocks(self.alloc.remove(first_gone, u64::MAX));
        match &mut self.store {
            Store::Heap(blocks) => {
                drop(blocks.split_off(&first_gone));
                // Zero the tail of the last block so a later extension reads zeroes there.
                if let Some(page) = blocks.get_mut(&(size / BLOCK)) {
                    page[(size % BLOCK) as usize..].fill(0);
                }
            }
            // The object keeps its length for the mappings that may reach past the new size.
            Store::Shared(pages) => {
                let len = pages.size();
                if len > size {
                    pages.punch_hole(size, len - size).map_err(host_errno)?;
                }
            }
        }
        self.size = size;
        Ok(())
    }

    /// Allocates the blocks under the byte range `start..end`, all or nothing: `ENOSPC` when
    /// the space cannot hold the ones not allocated yet (`shmem_fallocate`). With `grow`, the
    /// size becomes at least `end`.
    pub(crate) fn allocate(
        &mut self,
        space: &Space,
        start: u64,
        end: u64,
        grow: bool,
    ) -> FsResult<()> {
        if start < end {
            let (first, last) = (start / BLOCK, end.div_ceil(BLOCK));
            let missing = (last - first) - self.alloc.covered(first, last);
            space.charge_blocks(missing)?;
            self.alloc.insert(first, last);
        }
        if grow && end > self.size {
            self.extend(end)?;
            self.size = end;
        }
        Ok(())
    }

    /// Zeroes the byte range `start..end`, freeing the blocks it covers whole.
    pub(crate) fn punch(&mut self, space: &Space, start: u64, end: u64) -> FsResult<()> {
        if start >= end {
            return Ok(());
        }
        let (first_whole, last_whole) = (start.div_ceil(BLOCK), end / BLOCK);
        if first_whole < last_whole {
            space.release_blocks(self.alloc.remove(first_whole, last_whole));
        }
        let blocks = match &mut self.store {
            Store::Heap(blocks) => blocks,
            Store::Shared(pages) => {
                pages.punch_hole(start, end - start).map_err(host_errno)?;
                return Ok(());
            }
        };
        if first_whole < last_whole {
            let gone: Vec<u64> = blocks
                .range(first_whole..last_whole)
                .map(|(&index, _)| index)
                .collect();
            for index in gone {
                blocks.remove(&index);
            }
        }
        // The partial blocks at either end keep their allocation and lose their bytes.
        zero(blocks, start, end.min(first_whole * BLOCK));
        zero(blocks, start.max(last_whole * BLOCK), end);
        Ok(())
    }

    /// `SEEK_DATA`/`SEEK_HOLE`: blocks holding data are data, everything else (never written,
    /// or only allocated) is a hole, as tmpfs answers from its page cache.
    pub(crate) fn seek_hole(&self, offset: u64, which: SeekHole) -> FsResult<u64> {
        if offset >= self.size {
            return Err(Errno::ENXIO);
        }
        let blocks = match &self.store {
            Store::Heap(blocks) => blocks,
            Store::Shared(pages) => return self.seek_shared(&**pages, offset, which),
        };
        let index = offset / BLOCK;
        match which {
            SeekHole::Data => {
                let (&first, _) = blocks.range(index..).next().ok_or(Errno::ENXIO)?;
                let at = (first * BLOCK).max(offset);
                if at >= self.size {
                    return Err(Errno::ENXIO);
                }
                Ok(at)
            }
            SeekHole::Hole => {
                let mut next = index;
                for (&i, _) in blocks.range(index..) {
                    if i != next {
                        break;
                    }
                    next += 1;
                }
                Ok((next * BLOCK).max(offset).min(self.size))
            }
        }
    }

    /// `seek_hole` in a shared object, which answers for its pages; `offset` is inside the
    /// file. What lies past the object's length is a hole.
    fn seek_shared(&self, pages: &dyn SharedPages, offset: u64, which: SeekHole) -> FsResult<u64> {
        match which {
            SeekHole::Data => match pages.seek(offset, Seek::Data).map_err(host_errno)? {
                Some(at) if at < self.size => Ok(at),
                _ => Err(Errno::ENXIO),
            },
            SeekHole::Hole => {
                let at = pages.seek(offset, Seek::Hole).map_err(host_errno)?;
                Ok(at.unwrap_or(offset).min(self.size))
            }
        }
    }
}

/// Zeroes `start..end` of the heap blocks, which lies within one block.
fn zero(blocks: &mut BTreeMap<u64, Box<[u8]>>, start: u64, end: u64) {
    if start >= end {
        return;
    }
    if let Some(page) = blocks.get_mut(&(start / BLOCK)) {
        let at = (start % BLOCK) as usize;
        page[at..at + (end - start) as usize].fill(0);
    }
}

/// A set of block indices as disjoint, non-adjacent ranges `start..end`, keyed by start.
#[derive(Default)]
struct Ranges {
    ranges: BTreeMap<u64, u64>,
    /// Indices in the set.
    total: u64,
}

impl Ranges {
    fn contains(&self, index: u64) -> bool {
        self.ranges
            .range(..=index)
            .next_back()
            .is_some_and(|(_, &end)| end > index)
    }

    /// The ranges overlapping or touching `start..end`.
    fn near(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        let before = self
            .ranges
            .range(..start)
            .next_back()
            .filter(|(_, e)| **e >= start);
        before
            .into_iter()
            .chain(self.ranges.range(start..=end))
            .map(|(&s, &e)| (s, e))
            .collect()
    }

    /// How many of `start..end` are in the set.
    fn covered(&self, start: u64, end: u64) -> u64 {
        self.near(start, end)
            .into_iter()
            .map(|(s, e)| overlap(s, e, start, end))
            .sum()
    }

    /// Adds `start..end`; returns how many indices were new.
    fn insert(&mut self, start: u64, end: u64) -> u64 {
        let (mut from, mut to) = (start, end);
        let mut added = end - start;
        for (s, e) in self.near(start, end) {
            added -= overlap(s, e, start, end);
            from = from.min(s);
            to = to.max(e);
            self.ranges.remove(&s);
        }
        self.ranges.insert(from, to);
        self.total += added;
        added
    }

    /// Removes `start..end`; returns how many indices were in the set.
    fn remove(&mut self, start: u64, end: u64) -> u64 {
        let mut removed = 0;
        for (s, e) in self.near(start, end) {
            let n = overlap(s, e, start, end);
            if n == 0 {
                continue;
            }
            removed += n;
            self.ranges.remove(&s);
            if s < start {
                self.ranges.insert(s, start);
            }
            if e > end {
                self.ranges.insert(end, e);
            }
        }
        self.total -= removed;
        removed
    }
}

/// The size of the intersection of `a..b` and `c..d`.
fn overlap(a: u64, b: u64, c: u64, d: u64) -> u64 {
    b.min(d).saturating_sub(a.max(c))
}
