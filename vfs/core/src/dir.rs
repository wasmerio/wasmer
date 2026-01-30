//! Directory entry types.

use crate::{VfsFileType, VfsInodeId, VfsNameBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsDirEntry {
    pub name: VfsNameBuf,
    pub inode: Option<VfsInodeId>,
    pub file_type: Option<VfsFileType>,
}
