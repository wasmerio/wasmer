//! The interface filesystem drivers implement.
//!
//! Drivers store data. They never see paths, `.` or `..`, symlink resolution, credentials or
//! permission checks: the VFS walks paths one component at a time, decides every permission and
//! ownership question, and only then calls the driver. Mirrors the split between Linux's VFS and
//! a filesystem's `inode_operations`/`file_operations`.
//!
//! Everything is async so a driver can sit on the host's filesystem or the network. Futures are
//! not `Send`, like the syscalls that drive them.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::Errno;

use crate::types::{
    AllocMode, Attr, DirSink, FileType, FsInfo, FsResult, NewNode, OpenOptions, RenameMode,
    SeekHole, SetAttr, StatFs, XattrSet,
};

/// A mounted filesystem instance (Linux's `super_block`).
#[async_trait(?Send)]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FsInfo;

    /// The root directory. Available as soon as the filesystem is, so mounting needs no I/O.
    fn root(&self) -> Arc<dyn Node>;

    async fn statfs(&self) -> FsResult<StatFs>;

    /// Flushes everything to stable storage (`syncfs`).
    async fn sync(&self) -> FsResult<()> {
        Ok(())
    }
}

/// A filesystem object (Linux's `inode` plus its `inode_operations`).
///
/// Directory methods default to `ENOTDIR` and are only called on directories; names are single
/// components, never empty, `.` or `..`, never containing `/` or NUL, and at most
/// `FsInfo::name_max` bytes. `create`, `link` and `rename` get objects of the same filesystem.
///
/// Drivers keep the timestamps their own operations imply, as Linux filesystems do in their inode
/// operations: a write updates `mtime` and `ctime`, a new or removed entry updates the parent's,
/// `link`, `unlink` and `rename` update the `ctime` of the object whose link count or name
/// changed. Explicit changes come through `setattr`.
#[async_trait(?Send)]
pub trait Node: Send + Sync + Any {
    /// The inode number: stable for the object's lifetime and unique within the filesystem, and
    /// the object's identity. The VFS keeps one inode per number (its lock, its file locks, its
    /// directory entry), so two `Node`s with one number must be the same object, and a number
    /// must not name another object while a `Node` for the first is alive. A driver spanning
    /// several devices folds the device into the number.
    fn ino(&self) -> u64;

    fn file_type(&self) -> FileType;

    fn as_any(&self) -> &dyn Any;

    async fn getattr(&self) -> FsResult<Attr>;

    /// Applies `attr` and returns the resulting metadata. A size change on a regular file
    /// truncates or extends it.
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr>;

    /// The entry `name`; `ENOENT` if there is none.
    async fn lookup(&self, _name: &[u8]) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ENOTDIR)
    }

    /// Adds a new object under `name`; `EEXIST` if the name is taken.
    async fn create(&self, _name: &[u8], _new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        Err(Errno::ENOTDIR)
    }

    /// Creates an unnamed regular file in this directory for `O_TMPFILE`. Its link count is 0
    /// until `link` gives it a name.
    async fn create_unnamed(&self, _new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        Err(Errno::EOPNOTSUPP)
    }

    /// Adds `name` as another link to `target`, a non-directory.
    async fn link(&self, _name: &[u8], _target: &Arc<dyn Node>) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Removes the non-directory entry `name`.
    async fn unlink(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Removes the directory entry `name`; `ENOTEMPTY` unless it is empty.
    async fn rmdir(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Moves entry `name` to `new_name` in `new_dir` (possibly `self`). The VFS has already
    /// checked that the source exists, that types match for a replacement, that a directory is
    /// not moved under itself, and that `Exchange` has a target. Replacing a directory requires
    /// it to be empty (`ENOTEMPTY`). Unsupported modes return `EINVAL`.
    async fn rename(
        &self,
        _name: &[u8],
        _new_dir: &Arc<dyn Node>,
        _new_name: &[u8],
        _mode: RenameMode,
    ) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// The target of a symbolic link.
    async fn readlink(&self) -> FsResult<Vec<u8>> {
        Err(Errno::EINVAL)
    }

    /// Opens a regular file or directory for data access.
    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>>;

    /// The value of extended attribute `name` (full name, including the namespace prefix);
    /// `ENODATA` if it is not set.
    async fn getxattr(&self, _name: &[u8]) -> FsResult<Vec<u8>> {
        Err(Errno::EOPNOTSUPP)
    }

    async fn setxattr(&self, _name: &[u8], _value: &[u8], _how: XattrSet) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    /// Names of all extended attributes.
    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        Err(Errno::EOPNOTSUPP)
    }

    async fn removexattr(&self, _name: &[u8]) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }
}

/// An object opened for data access (Linux's `file_operations`).
///
/// Data methods are called on regular files only and `readdir` on directories only.
#[async_trait(?Send)]
pub trait Handle: Send + Sync {
    /// Reads at `offset`; 0 at or past the end.
    async fn read_at(&self, _buf: &mut [u8], _offset: u64) -> FsResult<usize> {
        Err(Errno::EISDIR)
    }

    /// Writes at `offset`, extending the file (with a hole) if it starts past the end.
    async fn write_at(&self, _data: &[u8], _offset: u64) -> FsResult<usize> {
        Err(Errno::EISDIR)
    }

    /// Writes at the current end of the file atomically with finding it (`O_APPEND`). Returns
    /// the offset written at and the count.
    async fn append(&self, _data: &[u8]) -> FsResult<(u64, usize)> {
        Err(Errno::EISDIR)
    }

    /// Flushes data, and metadata unless `data_only` (`fsync`/`fdatasync`).
    async fn sync(&self, _data_only: bool) -> FsResult<()> {
        Ok(())
    }

    async fn allocate(&self, _mode: AllocMode, _offset: u64, _len: u64) -> FsResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    /// The first offset at or after `offset` that starts data or a hole; `ENXIO` if `offset` is
    /// at or past the end. The default treats the whole file as data, as Linux's
    /// `generic_file_llseek` does.
    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        if offset >= size {
            return Err(Errno::ENXIO);
        }
        Ok(match which {
            SeekHole::Data => offset,
            SeekHole::Hole => size,
        })
    }

    /// Reads entries starting at `cookie` (0 is the start) into `sink` until it is full or the
    /// directory ends. Never produces `.` or `..`, which the VFS adds. Cookies are opaque, at
    /// most `MAX_DIR_COOKIE`, and resume correctly while entries are added and removed: an
    /// entry present for the whole read is produced exactly once, as tmpfs guarantees.
    async fn readdir(&self, _cookie: u64, _sink: &mut dyn DirSink) -> FsResult<()> {
        Err(Errno::ENOTDIR)
    }
}

/// Largest cookie a driver may hand out: `telldir` returns a `long`, 32 bits on wasm32, and the
/// VFS reserves 0..=2 for its own positions.
pub const MAX_DIR_COOKIE: u64 = i32::MAX as u64 - 3;
