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
//!   the last point merge byte by byte; where both changed a byte, the object wins.
//!
//! Bytes of a file mapping past the end of the file are never written back and keep what the
//! memory holds, as on Linux, where they live only in the last page cache page. Pages wholly
//! past the end read as zeroes instead of raising `SIGBUS`.
//!
//! Where synchronisation happens is described with the callers in `mm.rs` and `ipc/shm.rs`.

use std::{ops::Range, sync::Arc};

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
}

pub(crate) struct Shared {
    pub(crate) object: Object,
    /// Where in the object the mapping starts.
    pub(crate) offset: u64,
    /// The object's bytes under the mapping as of the last synchronisation.
    pub(crate) shadow: Vec<u8>,
    /// Whether memory changes reach the object: a shared file mapping of a descriptor not open
    /// for writing only ever reads, as Linux then clears `VM_SHARED`.
    pub(crate) writable: bool,
    /// For a writable shared file mapping, its count in the file's writable mappings, which
    /// `F_SEAL_WRITE` waits on.
    pub(crate) writer: Option<Writer>,
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
    /// many bytes lay inside the object.
    async fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, Errno> {
        match self {
            Object::File(file) => read_file(file, offset, buf).await,
            Object::Segment(attachment) => Ok(attachment.segment().read(offset, buf)),
        }
    }

    /// Writes the bytes of `data` that lie inside the object at `offset`.
    async fn write(&self, offset: u64, data: &[u8]) -> Result<(), Errno> {
        match self {
            Object::File(file) => write_file(file, offset, data).await,
            Object::Segment(attachment) => {
                attachment.segment().write(offset, data);
                Ok(())
            }
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
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut at = 0;
    while at < a.len() {
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

    pub(crate) fn shared(&self) -> Option<&Shared> {
        match &self.source {
            Source::Shared(shared) => Some(shared),
            Source::Private { .. } => None,
        }
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
                object: shared.object.clone(),
                offset: shared.offset + cut,
                shadow: shared.shadow.split_off(cut as usize),
                writable: shared.writable,
                writer: shared.writer.clone(),
            }),
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
            .read(at, &mut local)
            .await
            .map_err(|_| Errno::EFAULT)?;
        Ok(local)
    }

    /// Writes back what the memory changed since the last synchronisation, as `munmap`, `msync`
    /// and the end of the address space do.
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
            shared
                .object
                .write(shared.offset + run.start as u64, &local[run.clone()])
                .await?;
            shared.shadow[run.clone()].copy_from_slice(&local[run]);
        }
        Ok(())
    }

    /// Writes back the memory's changes, then copies in the object's.
    pub(crate) async fn sync(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        if self.shared().is_none() {
            return Ok(());
        }
        self.write_back(memory).await?;
        let local = self.snapshot(memory).await?;
        let Source::Shared(shared) = &mut self.source else {
            unreachable!("checked above");
        };
        let mut fresh = vec![0; self.len as usize];
        let inside = shared.object.read(shared.offset, &mut fresh).await?;
        // Past the end of the object the memory keeps what it holds.
        fresh[inside..].copy_from_slice(&local[inside..]);
        for run in differences(&fresh, &local) {
            let at = (self.start + run.start as u64) as u32;
            memory
                .write(at, &fresh[run])
                .await
                .map_err(|_| Errno::EFAULT)?;
        }
        shared.shadow = fresh;
        Ok(())
    }

    /// Fills the mapping's memory from its source, as a new mapping, or pages a private mapping
    /// dropped, read.
    pub(crate) async fn fill(&mut self, memory: &dyn UserMemory) -> Result<(), Errno> {
        let mut data = vec![0; self.len as usize];
        match &mut self.source {
            Source::Private { file, offset } => {
                read_file(file, *offset, &mut data).await?;
            }
            Source::Shared(shared) => {
                shared.object.read(shared.offset, &mut data).await?;
                shared.shadow = data.clone();
            }
        }
        let at = u32::try_from(self.start).map_err(|_| Errno::ENOMEM)?;
        memory.write(at, &data).await.map_err(|_| Errno::EFAULT)?;
        Ok(())
    }

    /// Grows the mapping by `extra` bytes at its end, which `mremap` already took, and fills
    /// them from the source.
    pub(crate) async fn grow(&mut self, memory: &dyn UserMemory, extra: u64) -> Result<(), Errno> {
        let mut tail = Mapping {
            start: self.end(),
            len: extra,
            source: match &self.source {
                Source::Private { file, offset } => Source::Private {
                    file: file.clone(),
                    offset: offset + self.len,
                },
                Source::Shared(shared) => Source::Shared(Shared {
                    object: shared.object.clone(),
                    offset: shared.offset + self.len,
                    shadow: Vec::new(),
                    writable: shared.writable,
                    writer: shared.writer.clone(),
                }),
            },
        };
        tail.fill(memory).await?;
        if let (Source::Shared(shared), Source::Shared(tail)) = (&mut self.source, tail.source) {
            shared.shadow.extend(tail.shadow);
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
    /// open for writing, or is write-sealed) overlaps `[start, end)`.
    pub(crate) fn read_only_shared_in(&self, start: u64, end: u64) -> bool {
        self.list.iter().any(|m| {
            m.start < end && m.end() > start && m.shared().is_some_and(|shared| !shared.writable)
        })
    }

    pub(crate) fn any_shared(&self) -> bool {
        self.list.iter().any(|m| m.shared().is_some())
    }

    /// The mapping that starts at `start`, if any.
    pub(crate) fn starting_at(&self, start: u64) -> Option<&Mapping> {
        self.list.iter().find(|m| m.start == start)
    }

    pub(crate) fn clear(&mut self) -> Vec<Mapping> {
        std::mem::take(&mut self.list)
    }

    /// `dup_mmap`'s copies for the fork child `pid`: the same files and shadows, and an
    /// attachment of its own for each segment the parent has attached (`shm_open`), shared by
    /// the parts of that attachment.
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
                        writable: shared.writable,
                        writer: shared.writer.clone(),
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
