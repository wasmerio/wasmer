//! Metadata types (`stat`-like).

use crate::{VfsFileType, VfsInodeId, VfsTimespec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsMetadata {
    pub inode: VfsInodeId,
    pub file_type: VfsFileType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    pub size: u64,
    pub atime: Option<VfsTimespec>,
    pub mtime: Option<VfsTimespec>,
    pub ctime: Option<VfsTimespec>,
    /// Backend-defined device id (for device nodes; otherwise usually 0).
    pub rdev: u64,
}
