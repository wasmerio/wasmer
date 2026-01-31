use vfs_core::{BackendInodeId, VfsFileType, VfsTimespec};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

#[derive(Debug)]
pub struct Stat {
    pub inode: BackendInodeId,
    pub file_type: VfsFileType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    pub size: u64,
    pub atime: VfsTimespec,
    pub mtime: VfsTimespec,
    pub ctime: VfsTimespec,
    pub rdev_major: u32,
    pub rdev_minor: u32,
    pub dir_handle: Option<DirHandle>,
}

#[derive(Debug)]
pub struct DirEntryInfo {
    pub name: Vec<u8>,
    pub inode: BackendInodeId,
    pub file_type: VfsFileType,
}
