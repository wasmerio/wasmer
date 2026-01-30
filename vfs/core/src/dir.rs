//! Directory entry types.

use crate::{VfsFileType, VfsInodeId, VfsPathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsDirEntry {
    pub name: VfsPathBuf,
    pub inode: Option<VfsInodeId>,
    pub file_type: Option<VfsFileType>,
}
