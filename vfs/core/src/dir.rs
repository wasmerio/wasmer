//! Directory entry types.

use crate::{BackendInodeId, VfsFileType, VfsNameBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsDirEntry {
    pub name: VfsNameBuf,
    pub inode: Option<BackendInodeId>,
    pub file_type: Option<VfsFileType>,
}
