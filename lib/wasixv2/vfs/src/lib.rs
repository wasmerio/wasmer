//! The single VFS: mount trees, path walking, name cache, permission checks, file locks, change
//! notification, and the inode-based trait that filesystem drivers implement.
//!
//! So far: the inode trait and a path walk over one tree. There are no mounts, symlinks, modes or
//! permission checks yet.

use std::sync::Arc;

use wasixv2_abi::Errno;
pub use wasixv2_host::LocalFuture;

/// Longest path, including its terminating NUL, that a syscall accepts.
pub const PATH_MAX: usize = 4096;
/// Longest single path component.
pub const NAME_MAX: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
}

/// A file or directory of one filesystem, as its driver implements it.
///
/// Data methods apply to regular files and default to `EISDIR`; namespace methods apply to
/// directories and default to `ENOTDIR`. Names are single components, never `.` or `..`.
pub trait Inode: Send + Sync {
    fn file_type(&self) -> FileType;

    /// Size in bytes.
    fn size(&self) -> u64 {
        0
    }

    /// Reads at `offset`; returns 0 at or past the end.
    fn read_at<'a>(
        &'a self,
        _buf: &'a mut [u8],
        _offset: u64,
    ) -> LocalFuture<'a, Result<usize, Errno>> {
        Box::pin(async { Err(Errno::EISDIR) })
    }

    /// Writes at `offset`, extending the file (with a hole) if `offset` is past the end.
    fn write_at<'a>(
        &'a self,
        _data: &'a [u8],
        _offset: u64,
    ) -> LocalFuture<'a, Result<usize, Errno>> {
        Box::pin(async { Err(Errno::EISDIR) })
    }

    /// Sets the size, discarding data past it or extending with zeroes.
    fn truncate(&self, _size: u64) -> Result<(), Errno> {
        Err(Errno::EISDIR)
    }

    /// The containing directory; `None` for the root.
    fn parent(&self) -> Option<Arc<dyn Inode>> {
        None
    }

    fn lookup(&self, _name: &[u8]) -> Result<Arc<dyn Inode>, Errno> {
        Err(Errno::ENOTDIR)
    }

    /// Creates an empty file or directory; `EEXIST` if the name is taken.
    fn create(&self, _name: &[u8], _file_type: FileType) -> Result<Arc<dyn Inode>, Errno> {
        Err(Errno::ENOTDIR)
    }

    /// Removes `name`, which must have type `file_type`, as `unlink` (regular) or `rmdir`
    /// (directory) do.
    fn remove(&self, _name: &[u8], _file_type: FileType) -> Result<(), Errno> {
        Err(Errno::ENOTDIR)
    }
}

/// A path split into the directory holding its last component and that component.
pub struct Parent {
    pub dir: Arc<dyn Inode>,
    /// `None` when the path ends in `.`, `..` or `/`, which name a directory and not an entry.
    pub name: Option<Vec<u8>>,
}

/// Walks every component of `path` but the last, from `root` for an absolute path and from `start`
/// otherwise.
pub fn walk_parent(
    root: &Arc<dyn Inode>,
    start: &Arc<dyn Inode>,
    path: &[u8],
) -> Result<Parent, Errno> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    let mut dir = if path[0] == b'/' {
        root.clone()
    } else {
        start.clone()
    };
    let mut components = path
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .peekable();
    while let Some(component) = components.next() {
        if component.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let last = components.peek().is_none();
        match component {
            b"." => {}
            b".." => dir = dir.parent().unwrap_or(dir),
            name if last && !path.ends_with(b"/") => {
                if dir.file_type() != FileType::Directory {
                    return Err(Errno::ENOTDIR);
                }
                return Ok(Parent {
                    dir,
                    name: Some(name.to_vec()),
                });
            }
            name => dir = dir.lookup(name)?,
        }
    }
    if dir.file_type() != FileType::Directory {
        return Err(Errno::ENOTDIR);
    }
    Ok(Parent { dir, name: None })
}

/// Resolves `path` to an existing inode, as `walk_parent` plus a lookup of the last component.
pub fn walk(
    root: &Arc<dyn Inode>,
    start: &Arc<dyn Inode>,
    path: &[u8],
) -> Result<Arc<dyn Inode>, Errno> {
    let parent = walk_parent(root, start, path)?;
    match parent.name {
        Some(name) => parent.dir.lookup(&name),
        None => Ok(parent.dir),
    }
}
