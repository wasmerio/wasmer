//! Filesystem node interfaces.
//!
//! Backend-facing `FsNode` trait for mount-agnostic VFS operations.

use crate::VfsFileType;
use crate::dir::VfsDirEntry;
use crate::flags::OpenOptions;
use crate::inode::make_vfs_inode;
use crate::path_types::{VfsName, VfsPath, VfsPathBuf};
use crate::{
    BackendInodeId, MountId, VfsError, VfsErrorKind, VfsInodeId, VfsMetadata, VfsResult,
    VfsSetMetadata,
};
use async_trait::async_trait;
use smallvec::SmallVec;
use std::sync::Arc;

/// Minimal node type classification (alias for now).
pub type VfsNodeKind = VfsFileType;

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

pub type SetMetadata = VfsSetMetadata;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VfsDirCookie(pub u64);

pub type DirCursor = VfsDirCookie;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadDirBatch {
    pub entries: SmallVec<[VfsDirEntry; 16]>,
    pub next: Option<VfsDirCookie>,
}

pub trait FsHandle: Send + Sync + 'static {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;
    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;

    fn flush(&self) -> VfsResult<()>;
    fn fsync(&self) -> VfsResult<()>;

    fn get_metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_handle.get_metadata",
        ))
    }

    fn set_len(&self, _len: u64) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_handle.set_len",
        ))
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

    fn is_seekable(&self) -> bool {
        true
    }
}

pub trait FsHandleSync: FsHandle {}

impl<T: FsHandle + ?Sized> FsHandleSync for T {}

#[async_trait]
pub trait FsHandleAsync: Send + Sync + 'static {
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;
    async fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;

    async fn flush(&self) -> VfsResult<()>;
    async fn fsync(&self) -> VfsResult<()>;

    async fn get_metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_handle_async.get_metadata",
        ))
    }

    async fn set_len(&self, _len: u64) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_handle_async.set_len",
        ))
    }

    async fn len(&self) -> VfsResult<u64> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_handle_async.len",
        ))
    }

    async fn append(&self, _buf: &[u8]) -> VfsResult<Option<usize>> {
        Ok(None)
    }

    async fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandleAsync>>> {
        Ok(None)
    }

    fn is_seekable(&self) -> bool {
        true
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
    fn read_dir(&self, cursor: Option<VfsDirCookie>, max: usize) -> VfsResult<ReadDirBatch>;

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
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_node.readlink",
        ))
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>>;
}

pub trait FsNodeSync: FsNode {}

impl<T: FsNode + ?Sized> FsNodeSync for T {}

#[async_trait]
pub trait FsNodeAsync: Send + Sync + 'static {
    fn inode(&self) -> BackendInodeId;
    fn file_type(&self) -> VfsFileType;

    async fn metadata(&self) -> VfsResult<VfsMetadata>;
    async fn set_metadata(&self, set: SetMetadata) -> VfsResult<()>;

    async fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNodeAsync>>;
    async fn create_file(
        &self,
        name: &VfsName,
        opts: CreateFile,
    ) -> VfsResult<Arc<dyn FsNodeAsync>>;
    async fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNodeAsync>>;
    async fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()>;
    async fn rmdir(&self, name: &VfsName) -> VfsResult<()>;
    async fn read_dir(&self, cursor: Option<VfsDirCookie>, max: usize) -> VfsResult<ReadDirBatch>;

    async fn rename(
        &self,
        old_name: &VfsName,
        new_parent: &dyn FsNodeAsync,
        new_name: &VfsName,
        opts: RenameOptions,
    ) -> VfsResult<()>;

    async fn link(&self, _existing: &dyn FsNodeAsync, _new_name: &VfsName) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_node_async.link",
        ))
    }

    async fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_node_async.symlink",
        ))
    }

    async fn readlink(&self) -> VfsResult<VfsPathBuf> {
        Err(VfsError::new(
            VfsErrorKind::NotSupported,
            "fs_node_async.readlink",
        ))
    }

    async fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandleAsync>>;
}

#[derive(Clone)]
pub struct VfsNode {
    pub id: VfsInodeId,
    pub inner: Arc<dyn FsNode>,
}

impl VfsNode {
    pub fn new(mount: MountId, inner: Arc<dyn FsNode>) -> Self {
        Self {
            id: make_vfs_inode(mount, inner.inode()),
            inner,
        }
    }
}
