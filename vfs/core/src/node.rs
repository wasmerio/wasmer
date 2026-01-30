//! Filesystem node interfaces.
//!
//! Backend-facing `FsNode` trait for mount-agnostic VFS operations.

use crate::dir::VfsDirEntry;
use crate::flags::OpenOptions;
use crate::path_types::{VfsName, VfsPath, VfsPathBuf};
use crate::{BackendInodeId, VfsError, VfsErrorKind, VfsMetadata, VfsResult};
use crate::{VfsFileType, VfsTimespec};
use smallvec::SmallVec;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub struct CreateFile {
    pub mode: Option<u32>,
    pub truncate: bool,
    pub exclusive: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MkdirOptions {
    pub mode: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnlinkOptions {
    pub must_be_dir: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenameOptions {
    pub noreplace: bool,
    pub exchange: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SetMetadata {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<VfsTimespec>,
    pub mtime: Option<VfsTimespec>,
    pub ctime: Option<VfsTimespec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirCursor(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadDirBatch {
    pub entries: SmallVec<[VfsDirEntry; 16]>,
    pub next: Option<DirCursor>,
}

pub trait FsHandle: Send + Sync + 'static {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;
    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;

    fn flush(&self) -> VfsResult<()>;
    fn fsync(&self) -> VfsResult<()>;

    fn set_len(&self, _len: u64) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "fs_handle.set_len"))
    }

    fn len(&self) -> VfsResult<u64> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "fs_handle.len"))
    }

    fn append(&self, _buf: &[u8]) -> VfsResult<Option<usize>> {
        Ok(None)
    }

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandle>>> {
        Ok(None)
    }
}

pub trait FsNode: Send + Sync + 'static {
    fn inode(&self) -> BackendInodeId;
    fn file_type(&self) -> VfsFileType;

    fn metadata(&self) -> VfsResult<VfsMetadata>;
    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()>;

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>>;
    fn create_file(&self, name: &VfsName, opts: CreateFile) -> VfsResult<Arc<dyn FsNode>>;
    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>>;
    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()>;
    fn rmdir(&self, name: &VfsName) -> VfsResult<()>;
    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch>;

    fn rename(
        &self,
        old_name: &VfsName,
        new_parent: &dyn FsNode,
        new_name: &VfsName,
        opts: RenameOptions,
    ) -> VfsResult<()>;

    fn link(&self, _existing: &dyn FsNode, _new_name: &VfsName) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "fs_node.link"))
    }

    fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "fs_node.symlink"))
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "fs_node.readlink"))
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>>;
}

