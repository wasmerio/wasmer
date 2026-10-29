//! File-backed mappings and shared memory in a linear memory that has no MMU.
//!
//! A mapping's pages are ordinary linear memory: they cannot alias a page cache page, a SysV
//! segment, or another mapping of the same object, because wasm can neither share part of a
//! memory between modules nor map one page at two addresses. So every mapping is a copy, and
//! what Linux gets from shared pages is rebuilt at defined points instead:
//!
//! - A **private** mapping (`MAP_PRIVATE`) is filled from the file when it is made, and again
//!   for pages `mremap` adds or `MADV_DONTNEED` drops. Linux leaves it unspecified whether a
//!   private mapping sees later changes to pages it never wrote; here it never does.
//! - A **shared** mapping (`MAP_SHARED` of a file, or a SysV segment attachment) keeps a shadow:
//!   the object's bytes as of the last synchronisation. Synchronising writes back every byte
//!   the memory changed since then (memory differs from the shadow), then copies in every byte
//!   the object changed (object differs from the shadow), so changes made on both sides since
//!   the last point merge byte by byte; where both changed a byte, the object wins. Comparing
//!   the memory with the shadow costs a read of the mapping each time; the object is only read
//!   again when it may have changed: always for a file, and for a segment only when somebody
//!   else wrote it since (it counts its writes).
//!
//! Bytes of a file mapping past the end of the file are never written back and keep what the
//! memory holds, as on Linux, where they live only in the last page cache page. Pages wholly
//! past the end read as zeroes instead of raising `SIGBUS`.
//!
//! Where synchronisation happens is described with the callers in `mm.rs` and `ipc/shm.rs`.
//!
//! A **mapped** object is the exception: where the host can place a shared object's pages
//! inside a linear memory ([`UserMemory::map_shared`]), a mapping of it is those very pages, as
//! every other mapping of it in any process is, and there is nothing to copy or synchronise. So
//! are `MAP_SHARED | MAP_ANONYMOUS` mappings (Linux's `shmem_zero_setup`), SysV segments, and
//! writable shared mappings of files whose filesystem keeps them in shared memory objects (the
//! memfd and `/dev/shm` tmpfs, [`wasixv2_vfs::Node::shared_pages`]). Making, moving and removing
//! such a mapping places the object's pages in the range and takes them out again.
//!
//! A mapping that may never be written (a `SHM_RDONLY` attachment, a shared mapping of a file
//! not open for writing or write-sealed) shows the pages read-only: the host maps them without
//! write access, as it does the low guard, so a store by the guest faults with `SIGSEGV` and a
//! syscall writing there fails with `EFAULT`. It still shows every store other mappings make.

use std::{ops::Range, sync::Arc};

use wasixv2_pages::SharedPages;
use wasixv2_syscalls::{Errno, UserMemory};

use wasixv2_vfs::Inode;

use crate::{file::OpenFile, ipc::shm::Attachment};

/// Differing runs closer than this are written as one, which costs less than another call.
const MERGE_GAP: usize = 32;

/// A mapping of something other than anonymous memory, at `start` for `len` bytes (whole
/// pages).
pub(crate) struct Mapping {
    pub(crate) start: u64,
    pub(crate) len: u64,
    pub(crate) source: Source,
}

pub(crate) enum Source {
    /// `MAP_PRIVATE` of a file, starting at `offset` in it.
    Private { file: Arc<OpenFile>, offset: u64 },
    /// A mapping the object's other users see changes of.
    Shared(Shared),
    /// A shared object whose pages the memory shows itself.
    Mapped(Mapped),
}

/// A mapping of a shared object's pages ([`UserMemory::map_shared`]).
#[derive(Clone)]
pub(crate) struct Mapped {
    pub(crate) pages: Arc<dyn SharedPages>,
    /// File whose newly materialized ranges must be charged on growth.
    pub(crate) file: Option<Arc<OpenFile>>,
    /// Where in the object the mapping starts.
    pub(crate) offset: u64,
    /// For a SysV segment attachment, the attachment it is a part of.
    pub(crate) segment: Option<Arc<Attachment>>,
    /// Whether the mapping may be written (`VM_MAYWRITE`): `SHM_RDONLY` clears it, and so does
    /// a file not open for writing or write-sealed. The memory shows the pages read-only then.
    pub(crate) writable: bool,
    /// For a shared mapping of a file, its count in the file's writable mappings, which
    /// `F_SEAL_WRITE` waits on; every part and fork copy counts on its own, as a vma does.
    pub(crate) writer: Option<Writer>,
}

impl Mapped {
    fn part(&self, skip: u64) -> Self {
        Self {
            offset: self.offset + skip,
            ..self.clone()
        }
    }

    /// Places `len` bytes of the object from the mapping's offset at `start`.
    async fn attach(&self, memory: &dyn UserMemory, start: u64, len: u64) -> Result<(), Errno> {
        let at = u32::try_from(start).map_err(|_| Errno::ENOMEM)?;
        memory
            .map_shared(at, len, &*self.pages, self.offset, self.writable)
            .await
    }
}

pub(crate) struct Shared {
    pub(crate) object: Object,
    /// Where in the object the mapping starts.
    pub(crate) offset: u64,
    /// The object's bytes under the mapping as of the last synchronisation.
    pub(crate) shadow: Vec<u8>,
    /// For an object that counts its writes (a segment), the count the shadow is of. While
    /// the object still has it, nobody changed the object since, and synchronising need not
    /// read it.
    generation: Option<u64>,
    /// Whether memory changes reach the object: a shared file mapping of a descriptor not open
    /// for writing only ever reads, as Linux then clears `VM_SHARED`.
    pub(crate) writable: bool,
    /// For a writable shared file mapping, its count in the file's writable mappings, which
    /// `F_SEAL_WRITE` waits on.
    pub(crate) writer: Option<Writer>,
}

impl Shared {
    pub(crate) fn new(object: Object, offset: u64, writable: bool, writer: Option<Writer>) -> Self {
        Self {
            object,
            offset,
            shadow: Vec::new(),
            generation: None,
            writable,
            writer,
        }
    }

    /// The same object from `skip` bytes further on, of which nothing is known yet.
    fn part(&self, skip: u64) -> Self {
        Self::new(
            self.object.clone(),
            self.offset + skip,
            self.writable,
            self.writer.clone(),
        )
    }
}

/// One writable shared mapping of a file, counted on its inode (`mapping_map_writable`) while it
/// exists.
pub(crate) struct Writer(Arc<Inode>);

impl Writer {
    /// Counts a new mapping of `inode`; `EPERM` once the file is write-sealed.
    pub(crate) fn new(inode: Arc<Inode>) -> Result<Self, Errno> {
        inode.map_writable()?;
        Ok(Self(inode))
    }
}

impl Clone for Writer {
    fn clone(&self) -> Self {
        self.0.allow_writable();
        Self(self.0.clone())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.0.unmap_writable();
    }
}

/// What a shared mapping shows.
#[derive(Clone)]
pub(crate) enum Object {
    File(Arc<OpenFile>),
    Segment(Arc<Attachment>),
}

impl Object {
    /// Fills `buf` from `offset`; bytes past the end of the object read as zero. Returns how
    /// many bytes lay inside the object, and its generation, if it counts them.
    async fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(usize, Option<u64>), Errno> {
        match self {
            Object::File(file) => Ok((read_file(file, offset, buf).await?, None)),
            Object::Segment(attachment) => attachment.segment().read(offset, buf),
        }
    }

    /// Writes the bytes of `data` that lie inside the object at `offset`. Returns the object's
    /// generation after the write, if it counts them.
    async fn write(&self, offset: u64, data: &[u8]) -> Result<Option<u64>, Errno> {
        match self {
            Object::File(file) => write_file(file, offset, data).await.map(|()| None),
            Object::Segment(attachment) => attachment.segment().write(offset, data),
        }
    }

    /// How many writes the object had, if it counts them.
    fn generation(&self) -> Option<u64> {
        match self {
            Object::File(_) => None,
            Object::Segment(attachment) => attachment.segment().generation(),
        }
    }

    /// The file behind a file mapping.
    pub(crate) fn file(&self) -> Option<&Arc<OpenFile>> {
        match self {
            Object::File(file) => Some(file),
            Object::Segment(_) => None,
        }
    }
}

/// Reads a file range, zero-filling past its end. Returns the bytes the file had.
pub(crate) async fn read_file(
    file: &OpenFile,
    offset: u64,
    buf: &mut [u8],
) -> Result<usize, Errno> {
    let inode = file.inode()?;
    let mut done = 0;
    while done < buf.len() {
        match inode
            .read_at(&mut buf[done..], offset + done as u64)
            .await?
        {
            0 => break,
            n => done += n,
        }
    }
    buf[done..].fill(0);
    Ok(done)
}

/// Writes back a file range, only up to the file's size: writeback never extends a file.
async fn write_file(file: &OpenFile, offset: u64, data: &[u8]) -> Result<(), Errno> {
    let inode = file.inode()?;
    let size = inode.node().getattr().await?.size;
    let end = size.saturating_sub(offset).min(data.len() as u64) as usize;
    let mut done = 0;
    while done < end {
        match inode
            .write_at(&data[done..end], offset + done as u64)
            .await?
        {
            0 => return Err(Errno::EIO),
            n => done += n,
        }
    }
    Ok(())
}

/// The runs where `a` and `b` differ, with nearby runs merged.
fn differences(a: &[u8], b: &[u8]) -> Vec<Range<usize>> {
    /// Equal stretches are skipped this many bytes at a time, which compiles to a `memcmp`.
    const STRIDE: usize = 64;
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    while at < a.len() {
        let stride = (at + STRIDE).min(a.len());
        if a[at..stride] == b[at..stride] {
            at = stride;
            continue;
        }
        if a[at] == b[at] {
            at += 1;
            continue;
        }
        let start = at;
        while at < a.len() && a[at] != b[at] {
            at += 1;
        }
        match runs.last_mut() {
            Some(last) if start - last.end < MERGE_GAP => last.end = at,
            _ => runs.push(start..at),
        }
    }
    runs
}

impl Mapping {
    pub(crate) fn end(&self) -> u64 {
        self.start + self.len
    }

    /// The synchronised side of a shared mapping kept as a copy.
    pub(crate) fn shared(&self) -> Option<&Shared> {
        match &self.source {
            Source::Shared(shared) => Some(shared),
            Source::Private { .. } | Source::Mapped(_) => None,
        }
    }

    pub(crate) fn mapped(&self) -> Option<&Mapped> {
        match &self.source {
            Source::Mapped(mapped) => Some(mapped),
            Source::Private { .. } | Source::Shared(_) => None,
        }
    }

    /// Whether the mapping shows an object other mappings share, copied or mapped.
    pub(crate) fn is_shared(&self) -> bool {
        !matches!(self.source, Source::Private { .. })
    }

    /// The SysV segment attachment the mapping is a part of, copied or mapped.
    pub(crate) fn attachment(&self) -> Option<&Arc<Attachment>> {
        match &self.source {
            Source::Shared(Shared {
                object: Object::Segment(attachment),
                ..
            }) => Some(attachment),
            Source::Mapped(mapped) => mapped.segment.as_ref(),
            Source::Shared(_) | Source::Private { .. } => None,
        }
    }

    /// What `munmap` does with the mapping's memory before the pages are free: a copied shared
    /// mapping is written back, and a mapped object's pages leave the range, which holds
    /// zeroed private pages again.
    pub(crate) async fn release(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        match &self.source {
            Source::Private { .. } => Ok(()),
            Source::Shared(_) => self.write_back(memory).await,
            Source::Mapped(_) => {
                let at = u32::try_from(self.start).map_err(|_| Errno::EFAULT)?;
                memory.unmap_shared(at, self.len).await
            }
        }
    }

    /// The mapping moved here from `from` (`mremap`), with its memory copied over. A mapped
    /// object's pages follow it: they are placed at the new address, and leave the old one.
    pub(crate) async fn relocate(
        &mut self,
        memory: &dyn UserMemory,
        from: u64,
    ) -> Result<(), Errno> {
        let Source::Mapped(mapped) = &self.source else {
            return Ok(());
        };
        mapped.attach(memory, self.start, self.len).await?;
        let at = u32::try_from(from).map_err(|_| Errno::EFAULT)?;
        memory.unmap_shared(at, self.len).await
    }

    /// Splits off the part from `at` on, which must lie inside the mapping.
    pub(crate) fn split_off(&mut self, at: u64) -> Mapping {
        let cut = at - self.start;
        let source = match &mut self.source {
            Source::Private { file, offset } => Source::Private {
                file: file.clone(),
                offset: *offset + cut,
            },
            Source::Shared(shared) => Source::Shared(Shared {
                shadow: shared.shadow.split_off(cut as usize),
                generation: shared.generation,
                ..shared.part(cut)
            }),
            Source::Mapped(mapped) => Source::Mapped(mapped.part(cut)),
        };
        let rest = Mapping {
            start: at,
            len: self.len - cut,
            source,
        };
        self.len = cut;
        rest
    }

    /// Reads the mapping's bytes out of memory.
    async fn snapshot(&self, memory: &dyn UserMemory) -> Result<Vec<u8>, Errno> {
        let mut local = vec![0; self.len as usize];
        let at = u32::try_from(self.start).map_err(|_| Errno::EFAULT)?;
        memory
            .force_read(at, &mut local)
            .await
            .map_err(|_| Errno::EFAULT)?;
        Ok(local)
    }

    /// Writes back what the memory changed since the last synchronisation, as `munmap`, `msync`
    /// and the end of the address space do. Afterwards a writable mapping's memory holds its
    /// shadow.
    pub(crate) async fn write_back(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        let Source::Shared(shared) = &self.source else {
            return Ok(());
        };
        if !shared.writable {
            return Ok(());
        }
        let local = self.snapshot(memory).await?;
        let Source::Shared(shared) = &mut self.source else {
            unreachable!("checked above");
        };
        for run in differences(&local, &shared.shadow) {
            let after = shared
                .object
                .write(shared.offset + run.start as u64, &local[run.clone()])
                .await?;
            // Only while each write is the next one the object had does it still match the
            // shadow everywhere else.
            shared.generation = match (shared.generation, after) {
                (Some(seen), Some(after)) if after == seen + 1 => Some(after),
                _ => None,
            };
            shared.shadow[run.clone()].copy_from_slice(&local[run]);
        }
        Ok(())
    }

    /// Writes back the memory's changes, then copies in the object's.
    pub(crate) async fn sync(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        let writable = shared.writable;
        self.write_back(memory).await?;
        // A mapping that never writes back may still differ from its shadow.
        let snapshot = match writable {
            true => None,
            false => Some(self.snapshot(memory).await?),
        };
        let (start, len) = (self.start, self.len as usize);
        let Source::Shared(shared) = &mut self.source else {
            unreachable!("checked above");
        };
        if shared.generation.is_some() && shared.generation == shared.object.generation() {
            return Ok(());
        }
        let mut fresh = vec![0; len];
        let (inside, generation) = shared.object.read(shared.offset, &mut fresh).await?;
        let local = snapshot.as_deref().unwrap_or(&shared.shadow);
        // Past the end of the object the memory keeps what it holds.
        fresh[inside..].copy_from_slice(&local[inside..]);
        for run in differences(&fresh, local) {
            let at = (start + run.start as u64) as u32;
            memory
                .force_write(at, &fresh[run])
                .await
                .map_err(|_| Errno::EFAULT)?;
        }
        shared.shadow = fresh;
        shared.generation = generation;
        Ok(())
    }

    /// Fills the mapping's memory from its source, as a new mapping, or pages a private mapping
    /// dropped, read. A mapped object's pages are placed in the range instead, which fails with
    /// `EOPNOTSUPP` where the memory cannot show them.
    pub(crate) async fn fill(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        let mut data = vec![0; self.len as usize];
        match &mut self.source {
            Source::Private { file, offset } => {
                read_file(file, *offset, &mut data).await?;
            }
            Source::Shared(shared) => {
                (_, shared.generation) = shared.object.read(shared.offset, &mut data).await?;
                shared.shadow = data.clone();
            }
            Source::Mapped(mapped) => return mapped.attach(memory, self.start, self.len).await,
        }
        let at = u32::try_from(self.start).map_err(|_| Errno::ENOMEM)?;
        memory
            .force_write(at, &data)
            .await
            .map_err(|_| Errno::EFAULT)?;
        Ok(())
    }

    /// A mapping of the same source as the part of this one from `skip` on, at `start` for
    /// `len` bytes (which may reach past this one's end), not filled yet.
    pub(crate) fn alias(&self, skip: u64, start: u64, len: u64) -> Mapping {
        let source = match &self.source {
            Source::Private { file, offset } => Source::Private {
                file: file.clone(),
                offset: offset + skip,
            },
            Source::Shared(shared) => Source::Shared(shared.part(skip)),
            Source::Mapped(mapped) => Source::Mapped(mapped.part(skip)),
        };
        Mapping { start, len, source }
    }

    /// Charge host pages before mremap changes any guest mappings. An alias may expose
    /// file ranges beyond the original mapping, including for old_size == 0.
    pub(crate) fn materialize(&mut self, cred: &wasixv2_vfs::Cred) -> Result<(), Errno> {
        if let Source::Mapped(mapped) = &mut self.source
            && let Some(file) = &mapped.file
            && let Some(pages) = file
                .inode()?
                .node()
                .shared_pages(mapped.offset..mapped.offset + self.len, cred)?
        {
            mapped.pages = pages;
        }
        Ok(())
    }

    /// Grows the mapping by `extra` bytes at its end, which `mremap` already took, and fills
    /// them from the source. A mapped object grows only as far as the object reaches: the
    /// pages past its end stay anonymous memory that reads zero, where Linux raises `SIGBUS`.
    pub(crate) async fn grow(&mut self, memory: &dyn UserMemory, extra: u64) -> Result<(), Errno> {
        let extra = match &self.source {
            Source::Mapped(mapped) => {
                let left = mapped.pages.size().saturating_sub(mapped.offset + self.len);
                extra.min(left)
            }
            Source::Private { .. } | Source::Shared(_) => extra,
        };
        if extra == 0 {
            return Ok(());
        }
        let mut tail = self.alias(self.len, self.end(), extra);
        tail.fill(memory).await?;
        if let (Source::Shared(shared), Source::Shared(tail)) = (&mut self.source, tail.source) {
            shared.shadow.extend(tail.shadow);
            shared.generation = shared
                .generation
                .filter(|&seen| tail.generation == Some(seen));
        }
        self.len += extra;
        Ok(())
    }
}

/// The mappings of an address space, in address order and never overlapping.
#[derive(Default)]
pub(crate) struct Mappings {
    list: Vec<Mapping>,
}

impl Mappings {
    /// Where each mapping starts and ends, and whether it is shared, lowest first.
    pub(crate) fn spans(&self) -> Vec<(u64, u64, bool)> {
        self.list
            .iter()
            .map(|m| (m.start, m.end(), m.is_shared()))
            .collect()
    }

    pub(crate) fn insert(&mut self, mapping: Mapping) {
        let at = self.list.partition_point(|m| m.start < mapping.start);
        self.list.insert(at, mapping);
    }

    /// Takes out the parts of mappings inside `[start, end)`, splitting those that straddle it.
    pub(crate) fn take_range(&mut self, start: u64, end: u64) -> Vec<Mapping> {
        let mut taken = Vec::new();
        let mut kept = Vec::with_capacity(self.list.len());
        for mut mapping in self.list.drain(..) {
            if mapping.end() <= start || mapping.start >= end {
                kept.push(mapping);
                continue;
            }
            if mapping.start < start {
                let rest = mapping.split_off(start);
                kept.push(mapping);
                mapping = rest;
            }
            if mapping.end() > end {
                let rest = mapping.split_off(end);
                kept.push(rest);
            }
            taken.push(mapping);
        }
        kept.sort_by_key(|m| m.start);
        self.list = kept;
        taken
    }

    /// Takes out every mapping `pick` selects.
    pub(crate) fn take_where(&mut self, mut pick: impl FnMut(&Mapping) -> bool) -> Vec<Mapping> {
        let (taken, kept) = self.list.drain(..).partition(|m| pick(m));
        self.list = kept;
        taken
    }

    /// Puts back mappings taken out, at their (unchanged) addresses.
    pub(crate) fn restore(&mut self, mappings: Vec<Mapping>) {
        for mapping in mappings {
            self.insert(mapping);
        }
    }

    /// Whether a shared mapping that may never be written (`VM_MAYWRITE` clear: its file is not
    /// open for writing, or is write-sealed, or it is a `SHM_RDONLY` attachment) overlaps
    /// `[start, end)`.
    pub(crate) fn read_only_shared_in(&self, start: u64, end: u64) -> bool {
        let read_only = |m: &Mapping| match &m.source {
            Source::Shared(shared) => !shared.writable,
            Source::Mapped(mapped) => !mapped.writable,
            Source::Private { .. } => false,
        };
        self.list
            .iter()
            .any(|m| m.start < end && m.end() > start && read_only(m))
    }

    pub(crate) fn any_shared(&self) -> bool {
        self.list.iter().any(|m| m.shared().is_some())
    }

    /// The mappings that show shared objects' pages, lowest first.
    pub(crate) fn mapped(&self) -> impl Iterator<Item = (&Mapping, &Mapped)> {
        self.list
            .iter()
            .filter_map(|m| m.mapped().map(|mapped| (m, mapped)))
    }

    /// The lowest mapping that overlaps `[start, end)`, if any.
    pub(crate) fn first_in(&self, start: u64, end: u64) -> Option<&Mapping> {
        self.list.iter().find(|m| m.start < end && m.end() > start)
    }

    /// The mapping `addr` lies in, if any.
    pub(crate) fn containing(&self, addr: u64) -> Option<&Mapping> {
        self.list.iter().find(|m| m.start <= addr && addr < m.end())
    }

    /// Unfilled mappings of the same sources as the parts of mappings inside `[start, end)`, at
    /// the same addresses.
    pub(crate) fn aliases(&self, start: u64, end: u64) -> Vec<Mapping> {
        self.list
            .iter()
            .filter(|m| m.start < end && m.end() > start)
            .map(|m| {
                let (from, to) = (m.start.max(start), m.end().min(end));
                m.alias(from - m.start, from, to - from)
            })
            .collect()
    }

    /// The mapping that starts at `start`, if any.
    pub(crate) fn starting_at(&self, start: u64) -> Option<&Mapping> {
        self.list.iter().find(|m| m.start == start)
    }

    pub(crate) fn clear(&mut self) -> Vec<Mapping> {
        std::mem::take(&mut self.list)
    }

    /// `dup_mmap`'s copies for the fork child `pid`: the same files, shadows and mapped objects,
    /// and an attachment of its own for each segment the parent has attached (`shm_open`),
    /// shared by the parts of that attachment. The child's memory shows the mapped objects
    /// again once the execution layer copied it ([`wasixv2_syscalls::SharedMappings`]).
    pub(crate) fn dup(&self, pid: u32) -> Self {
        let mut attachments: Vec<(Arc<Attachment>, Arc<Attachment>)> = Vec::new();
        let mut segment = |parent: &Arc<Attachment>| {
            if let Some((_, child)) = attachments.iter().find(|(p, _)| Arc::ptr_eq(p, parent)) {
                return child.clone();
            }
            let child = parent.dup(pid);
            attachments.push((parent.clone(), child.clone()));
            child
        };
        let list = self
            .list
            .iter()
            .map(|mapping| Mapping {
                start: mapping.start,
                len: mapping.len,
                source: match &mapping.source {
                    Source::Private { file, offset } => Source::Private {
                        file: file.clone(),
                        offset: *offset,
                    },
                    Source::Shared(shared) => Source::Shared(Shared {
                        object: match &shared.object {
                            Object::File(file) => Object::File(file.clone()),
                            Object::Segment(parent) => Object::Segment(segment(parent)),
                        },
                        offset: shared.offset,
                        shadow: shared.shadow.clone(),
                        generation: shared.generation,
                        writable: shared.writable,
                        writer: shared.writer.clone(),
                    }),
                    Source::Mapped(mapped) => Source::Mapped(Mapped {
                        segment: mapped.segment.as_ref().map(&mut segment),
                        ..mapped.clone()
                    }),
                },
            })
            .collect();
        Self { list }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_merge_close_runs() {
        let a = [0u8; 100];
        let mut b = [0u8; 100];
        b[3] = 1;
        b[10] = 1;
        b[80] = 1;
        assert_eq!(differences(&a, &b), [3..11, 80..81]);
        assert!(differences(&a, &a).is_empty());
    }
}
