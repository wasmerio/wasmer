//! Open files: the filesystem part of an open file description.

use std::{sync::Arc, time::Duration};

use wasixv2_abi::Errno;

use crate::{
    Vfs,
    driver::{Handle, Node, SharedContents},
    inode::OpenAccess,
    locks::{LeaseHolder, LeaseRequest, LockKey, LockKind},
    mount::{MntNs, Path, WriteAccess},
    notify::{FsEvent, Object},
    types::{DirEntry, DirSink, FileType, FsResult},
};

/// The directory position after the last entry (tmpfs's `DIR_OFFSET_EOD`). Entries added after a
/// reader reached the end are not returned until it seeks back.
pub const DIR_POS_EOD: u64 = i32::MAX as u64;

/// An opened object. Shared by every descriptor of one open file description; dropping the last
/// reference is the last close.
pub struct File {
    pub path: Path,
    handle: Option<Box<dyn Handle>>,
    /// Unique per open, the owner of `flock` and OFD locks.
    pub id: u64,
    pub readable: bool,
    pub writable: bool,
    /// Opened with `O_PATH`: only usable as a location.
    pub path_only: bool,
    /// The open created the file (`FMODE_CREATED`), which `F_CREATED_QUERY` reports.
    pub created: bool,
    vfs: Arc<Vfs>,
    /// The write access a file open for writing holds on its mount (`FMODE_WRITER`).
    _write: Option<WriteAccess>,
    /// What the file counts on its inode: write access, or a read-only open.
    _access: Option<OpenAccess>,
    /// The anonymous namespace of the detached tree whose root this is (`FMODE_NEED_UNMOUNT`):
    /// the last close unmounts the tree unless it was attached somewhere since
    /// (`dissolve_on_fput`).
    detached: Option<Arc<MntNs>>,
}

impl File {
    pub(crate) fn new(
        vfs: Arc<Vfs>,
        path: Path,
        handle: Option<Box<dyn Handle>>,
        readable: bool,
        write: Option<WriteAccess>,
        writable: bool,
        path_only: bool,
    ) -> Self {
        Self {
            id: vfs.next_id(),
            path,
            handle,
            readable,
            writable,
            path_only,
            created: false,
            vfs,
            _write: write,
            _access: None,
            detached: None,
        }
    }

    /// The file with `access` counted on its inode for as long as it is open.
    pub(crate) fn with_access(mut self, access: Option<OpenAccess>) -> Self {
        self._access = access;
        self
    }

    /// An `O_PATH` file at the root of the detached tree in `ns`, which it keeps until its
    /// last close.
    pub(crate) fn detached(vfs: Arc<Vfs>, ns: Arc<MntNs>) -> Self {
        let mut file = Self::new(vfs, ns.root(), None, false, None, false, true);
        file.detached = Some(ns);
        file
    }

    pub fn node(&self) -> &Arc<dyn Node> {
        self.path.dentry.node()
    }

    pub fn file_type(&self) -> FileType {
        self.node().file_type()
    }

    pub fn vfs(&self) -> &Arc<Vfs> {
        &self.vfs
    }

    /// The key this object's locks are filed under.
    pub fn lock_key(&self) -> LockKey {
        (self.path.dentry.sb().id, self.node().ino())
    }

    pub(crate) fn object(&self) -> Object {
        Object {
            sb: self.path.dentry.sb().id,
            ino: self.node().ino(),
        }
    }

    fn handle(&self) -> FsResult<&dyn Handle> {
        match (&self.handle, self.file_type()) {
            (Some(handle), _) => Ok(handle.as_ref()),
            (None, FileType::Directory) => Err(Errno::EISDIR),
            (None, _) => Err(Errno::EINVAL),
        }
    }

    /// Reads at `offset` (`vfs_read` after the access-mode check).
    pub async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let handle = self.handle()?;
        let n = handle.read_at(buf, offset).await?;
        self.vfs.notify_object(
            &self.path.dentry,
            FsEvent::Access {
                object: self.object(),
            },
        );
        Ok(n)
    }

    /// The file's contents as its driver shares them, if it does ([`Handle::shared_contents`]):
    /// a read of the whole file without a copy.
    pub async fn shared_contents(&self) -> Option<SharedContents> {
        let contents = self.handle().ok()?.shared_contents().await?;
        self.vfs.notify_object(
            &self.path.dentry,
            FsEvent::Access {
                object: self.object(),
            },
        );
        Some(contents)
    }

    /// Writes at `offset`, as much of `data` as the file's seals allow.
    pub async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        let handle = self.handle()?;
        let inode = self.path.dentry.inode();
        let _sealed = inode.seal_lock().await;
        let data = inode.sealed_write(data, Some(offset)).await?;
        let n = handle.write_at(data, offset).await?;
        self.modified(n);
        Ok(n)
    }

    /// Writes at the end of the file (`O_APPEND`). Returns the offset after the data.
    pub async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        let handle = self.handle()?;
        let inode = self.path.dentry.inode();
        let _sealed = inode.seal_lock().await;
        let data = inode.sealed_write(data, None).await?;
        let (at, n) = handle.append(data).await?;
        self.modified(n);
        Ok((at + n as u64, n))
    }

    /// `F_SETLEASE` with `F_RDLCK` or `F_WRLCK` (`generic_add_lease`): takes or changes the
    /// file's lease, which `holder` keeps. `EAGAIN` while the file is open in a way the lease
    /// would hide (a read lease needs no writer, a write lease no other opener), or another
    /// lease is in the way; a new lease needs `enabled` (`fs.leases-enable`), else `EINVAL`.
    /// `now` is the clock of [`crate::LeaseWait`].
    pub fn set_lease(
        &self,
        kind: LockKind,
        holder: Arc<dyn LeaseHolder>,
        enabled: bool,
        now: Duration,
    ) -> FsResult<()> {
        // `lease_open_conflict`: the file's own part of the inode's counts.
        let own = match (self.writable, self.readable) {
            (true, _) => (1, 0),
            (false, true) => (0, 1),
            (false, false) => (0, 0),
        };
        let request = LeaseRequest {
            file: self.id,
            kind,
            own,
            inode: self.path.dentry.inode(),
            holder,
            enabled,
            now,
        };
        self.vfs.locks.add_lease(self.lock_key(), request)
    }

    /// `F_SETLEASE` with `F_UNLCK` (`generic_delete_lease`): `EAGAIN` without a lease.
    pub fn remove_lease(&self, now: Duration) -> FsResult<()> {
        self.vfs.locks.remove_lease(self.lock_key(), self.id, now)
    }

    /// `F_GETLEASE`: the file's lease as it is to become once broken, `None` for `F_UNLCK`.
    pub fn lease(&self, now: Duration) -> Option<LockKind> {
        self.vfs.locks.lease(self.lock_key(), self.id, now)
    }

    fn modified(&self, n: usize) {
        if n > 0 {
            self.vfs.notify_object(
                &self.path.dentry,
                FsEvent::Modify {
                    object: self.object(),
                },
            );
        }
    }

    pub async fn sync(&self, data_only: bool) -> FsResult<()> {
        match &self.handle {
            Some(handle) => handle.sync(data_only).await,
            None => Ok(()),
        }
    }

    pub async fn allocate(&self, mode: crate::AllocMode, offset: u64, len: u64) -> FsResult<()> {
        let handle = self.handle()?;
        let inode = self.path.dentry.inode();
        let _sealed = inode.seal_lock().await;
        inode.sealed_allocate(mode, offset, len).await?;
        handle.allocate(mode, offset, len).await?;
        self.modified(1);
        Ok(())
    }

    /// `ftruncate` (`do_truncate` with the open file): sets the size through this open file, so
    /// only its write access counts, never the permission bits. The caller has checked that it
    /// is a regular file opened for writing.
    pub async fn set_size(&self, size: u64) -> FsResult<()> {
        let handle = self.handle()?;
        let inode = self.path.dentry.inode();
        let _sealed = inode.seal_lock().await;
        let attr = crate::SetAttr {
            size: Some(size),
            ..crate::SetAttr::default()
        };
        inode.sealed_setattr(&attr).await?;
        handle.set_size(size).await?;
        self.modified(1);
        Ok(())
    }

    pub async fn seek_hole(&self, offset: u64, which: crate::SeekHole) -> FsResult<u64> {
        let size = self.node().getattr().await?.size;
        self.handle()?.seek_hole(offset, size, which).await
    }

    /// Reads directory entries from position `pos` into `sink`, `.` and `..` first, and returns
    /// the position after the last entry taken (`iterate_dir` with `dir_emit_dots`).
    pub async fn readdir(&self, pos: u64, sink: &mut dyn DirSink) -> FsResult<u64> {
        if self.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        let handle = self.handle.as_deref().ok_or(Errno::EBADF)?;
        let mut pos = pos;
        if pos == 0 {
            let dot = DirEntry {
                name: b".",
                ino: self.node().ino(),
                file_type: FileType::Directory,
                next: 1,
            };
            if !sink.push(dot) {
                return Ok(pos);
            }
            pos = 1;
        }
        if pos == 1 {
            let parent_ino = self
                .path
                .dentry
                .parent()
                .map_or(self.node().ino(), |parent| parent.node().ino());
            let dotdot = DirEntry {
                name: b"..",
                ino: parent_ino,
                file_type: FileType::Directory,
                next: 2,
            };
            if !sink.push(dotdot) {
                return Ok(pos);
            }
            pos = 2;
        }
        if pos >= DIR_POS_EOD {
            return Ok(pos);
        }
        let mut shifted = Shifted {
            inner: sink,
            pos,
            full: false,
        };
        handle.readdir(pos - 2, &mut shifted).await?;
        Ok(if shifted.full {
            shifted.pos
        } else {
            DIR_POS_EOD
        })
    }
}

/// Maps driver cookies into the position space after `.` and `..`.
struct Shifted<'a> {
    inner: &'a mut dyn DirSink,
    pos: u64,
    full: bool,
}

impl DirSink for Shifted<'_> {
    fn push(&mut self, entry: DirEntry<'_>) -> bool {
        // A name no lookup could reach would corrupt the listing (`..`, `a/b`, or one longer
        // than a record can say): a driver's bad entry is skipped.
        if !is_entry_name(entry.name) {
            return true;
        }
        let next = (entry.next + 2).min(DIR_POS_EOD - 1);
        if !self.inner.push(DirEntry { next, ..entry }) {
            self.full = true;
            return false;
        }
        self.pos = next;
        true
    }
}

/// A name a directory entry can have.
fn is_entry_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= crate::NAME_MAX
        && name != b"."
        && name != b".."
        && !name.iter().any(|&b| b == b'/' || b == 0)
}

impl Drop for File {
    fn drop(&mut self) {
        if self.path_only {
            return;
        }
        self.vfs.locks.release_file(self.lock_key(), self.id);
        self.vfs.notify_object(
            &self.path.dentry,
            FsEvent::Close {
                object: self.object(),
                wrote: self.writable,
            },
        );
    }
}
