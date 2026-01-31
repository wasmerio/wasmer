//! Async backend traits for the VFS layer.
//!
//! All async traits use `async_trait` for object safety and require `Send + Sync`.

use crate::flags::OpenOptions;
use crate::node::AsAny;
use crate::path_types::{VfsName, VfsPath, VfsPathBuf};
use crate::{BackendInodeId, VfsError, VfsErrorKind, VfsFileType, VfsMetadata, VfsResult};
use async_trait::async_trait;
use std::sync::Arc;

use crate::node::{
    CreateFile, DirCursor, MkdirOptions, ReadDirBatch, RenameOptions, SetMetadata, UnlinkOptions,
};

/// Async handle trait.
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

/// Async node trait.
#[async_trait]
pub trait FsNodeAsync: Send + Sync + 'static + AsAny {
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
    async fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch>;

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

/// Async filesystem instance.
#[async_trait]
pub trait FsAsync: Send + Sync + 'static {
    fn provider_name(&self) -> &'static str;
    fn capabilities(&self) -> crate::VfsCapabilities;

    async fn root(&self) -> VfsResult<Arc<dyn FsNodeAsync>>;

    async fn node_by_inode(
        &self,
        inode: BackendInodeId,
    ) -> VfsResult<Option<Arc<dyn FsNodeAsync>>> {
        let _ = inode;
        Ok(None)
    }
}

/// Async filesystem provider.
#[async_trait]
pub trait FsProviderAsync: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> crate::provider::FsProviderCapabilities;

    fn validate_config(&self, _config: &dyn crate::provider::ProviderConfig) -> VfsResult<()> {
        Ok(())
    }

    async fn mount(&self, req: crate::provider::MountRequest<'_>) -> VfsResult<Arc<dyn FsAsync>>;
}
