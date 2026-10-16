//! A regular file's contents: written blocks, and the ranges of blocks allocated to it.
//!
//! Allocation is kept as ranges, so `fallocate`, hole punching and truncation cost the number of
//! ranges and stored blocks they touch, never the length of the byte range: allocating a
//! terabyte is one range. Blocks `fallocate` allocated hold no memory until they are written,
//! and read as zeroes; like tmpfs's not-yet-uptodate pages they count as space used but are
//! holes to `SEEK_DATA`/`SEEK_HOLE`.

use std::collections::BTreeMap;

use wasixv2_abi::Errno;
use wasixv2_vfs::{FsResult, SeekHole};

use crate::{BLOCK, MAX_SIZE, Space};

/// The contents of a regular file. Every block in `pages` is in `alloc`.
#[derive(Default)]
pub(crate) struct Data {
    pub(crate) size: u64,
    /// Blocks holding data, by index.
    pages: BTreeMap<u64, Box<[u8]>>,
    /// Blocks charged to the filesystem's space: written ones and those `fallocate` reserved.
    alloc: Ranges,
}

impl Data {
    /// Blocks charged to this file.
    pub(crate) fn allocated(&self) -> u64 {
        self.alloc.total
    }

    pub(crate) fn read(&self, buf: &mut [u8], offset: u64) -> usize {
        let end = self.size.min(offset.saturating_add(buf.len() as u64));
        let mut pos = offset;
        while pos < end {
            let (index, start) = (pos / BLOCK, (pos % BLOCK) as usize);
            let len = ((BLOCK - start as u64).min(end - pos)) as usize;
            let out = &mut buf[(pos - offset) as usize..][..len];
            match self.pages.get(&index) {
                Some(page) => out.copy_from_slice(&page[start..start + len]),
                None => out.fill(0),
            }
            pos += len as u64;
        }
        end.saturating_sub(offset) as usize
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
        if pos == offset {
            return Err(Errno::ENOSPC);
        }
        self.size = self.size.max(pos);
        Ok((pos - offset) as usize)
    }

    /// The block `index`, allocated and charged if it holds no data yet; `None` if the space is
    /// full.
    fn page(&mut self, space: &Space, index: u64) -> Option<&mut Box<[u8]>> {
        if !self.pages.contains_key(&index) {
            if !self.alloc.contains(index) {
                space.charge_blocks(1).ok()?;
                self.alloc.insert(index, index + 1);
            }
            self.pages
                .insert(index, vec![0; BLOCK as usize].into_boxed_slice());
        }
        self.pages.get_mut(&index)
    }

    /// Sets the size. Shrinking frees every block past the new end, including blocks allocated
    /// past the old end with `FALLOC_FL_KEEP_SIZE` (`shmem_setattr`); growing leaves a hole.
    pub(crate) fn truncate(&mut self, space: &Space, size: u64) {
        if size < self.size {
            let first_gone = size.div_ceil(BLOCK);
            drop(self.pages.split_off(&first_gone));
            space.release_blocks(self.alloc.remove(first_gone, u64::MAX));
            // Zero the tail of the last block so a later extension reads zeroes there.
            if let Some(page) = self.pages.get_mut(&(size / BLOCK)) {
                page[(size % BLOCK) as usize..].fill(0);
            }
        }
        self.size = size;
    }

    /// Allocates the blocks under the byte range `start..end`, all or nothing: `ENOSPC` when
    /// the space cannot hold the ones not allocated yet (`shmem_fallocate`).
    pub(crate) fn allocate(&mut self, space: &Space, start: u64, end: u64) -> FsResult<()> {
        if start >= end {
            return Ok(());
        }
        let (first, last) = (start / BLOCK, end.div_ceil(BLOCK));
        let missing = (last - first) - self.alloc.covered(first, last);
        space.charge_blocks(missing)?;
        self.alloc.insert(first, last);
        Ok(())
    }

    /// Zeroes the byte range `start..end`, freeing the blocks it covers whole.
    pub(crate) fn punch(&mut self, space: &Space, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let (first_whole, last_whole) = (start.div_ceil(BLOCK), end / BLOCK);
        if first_whole < last_whole {
            let gone: Vec<u64> = self
                .pages
                .range(first_whole..last_whole)
                .map(|(&index, _)| index)
                .collect();
            for index in gone {
                self.pages.remove(&index);
            }
            space.release_blocks(self.alloc.remove(first_whole, last_whole));
        }
        // The partial blocks at either end keep their allocation and lose their bytes.
        self.zero(start, end.min(first_whole * BLOCK));
        self.zero(start.max(last_whole * BLOCK), end);
    }

    /// Zeroes `start..end`, which lies within one block.
    fn zero(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        if let Some(page) = self.pages.get_mut(&(start / BLOCK)) {
            let at = (start % BLOCK) as usize;
            page[at..at + (end - start) as usize].fill(0);
        }
    }

    /// `SEEK_DATA`/`SEEK_HOLE`: blocks holding data are data, everything else (never written,
    /// or only allocated) is a hole, as tmpfs answers from its page cache.
    pub(crate) fn seek_hole(&self, offset: u64, which: SeekHole) -> FsResult<u64> {
        if offset >= self.size {
            return Err(Errno::ENXIO);
        }
        let index = offset / BLOCK;
        match which {
            SeekHole::Data => {
                let (&first, _) = self.pages.range(index..).next().ok_or(Errno::ENXIO)?;
                let at = (first * BLOCK).max(offset);
                if at >= self.size {
                    return Err(Errno::ENXIO);
                }
                Ok(at)
            }
            SeekHole::Hole => {
                let mut next = index;
                for (&i, _) in self.pages.range(index..) {
                    if i != next {
                        break;
                    }
                    next += 1;
                }
                Ok((next * BLOCK).max(offset).min(self.size))
            }
        }
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
