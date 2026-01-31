//! Sync backend traits for the VFS layer.
//!
//! These traits are the canonical sync interface. Type aliases `FsNode` and `FsHandle`
//! in `node.rs` refer to these for backward compatibility.

use crate::flags::OpenOptions;
use crate::node::AsAny;
use crate::path_types::{VfsName, VfsPath, VfsPathBuf};
use crate::{BackendInodeId, VfsError, VfsErrorKind, VfsFileType, VfsMetadata, VfsResult};
use std::sync::Arc;

use crate::node::{
    CreateFile, DirCursor, MkdirOptions, ReadDirBatch, RenameOptions, SetMetadata, UnlinkOptions,
};

/// Sync handle trait: read_at, write_at, flush, fsync, etc.
pub trait FsHandleSync: Send + Sync + 'static {
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

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandleSync>>> {
        Ok(None)
    }

    fn is_seekable(&self) -> bool {
        true
    }
}

/// Sync node trait: lookup, create_file, mkdir, open, etc.
pub trait FsNodeSync: Send + Sync + 'static + AsAny {
    fn inode(&self) -> BackendInodeId;
    fn file_type(&self) -> VfsFileType;

    fn metadata(&self) -> VfsResult<VfsMetadata>;
    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()>;

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNodeSync>>;
    fn create_file(&self, name: &VfsName, opts: CreateFile) -> VfsResult<Arc<dyn FsNodeSync>>;
    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNodeSync>>;
    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()>;
    fn rmdir(&self, name: &VfsName) -> VfsResult<()>;
    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch>;

    fn rename(
        &self,
        old_name: &VfsName,
        new_parent: &dyn FsNodeSync,
        new_name: &VfsName,
        opts: RenameOptions,
    ) -> VfsResult<()>;

    fn link(&self, _existing: &dyn FsNodeSync, _new_name: &VfsName) -> VfsResult<()> {
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

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandleSync>>;
}

/// Sync filesystem instance (superblock-like).
pub trait FsSync: Send + Sync + 'static {
    fn provider_name(&self) -> &'static str;
    fn capabilities(&self) -> crate::VfsCapabilities;

    fn root(&self) -> Arc<dyn FsNodeSync>;

    fn node_by_inode(&self, _inode: BackendInodeId) -> Option<Arc<dyn FsNodeSync>> {
        None
    }
}

/// Sync filesystem provider (registry-visible, creates filesystem instances).
pub trait FsProviderSync: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> crate::provider::FsProviderCapabilities;

    fn provider_capabilities(&self) -> crate::provider::FsProviderCapabilities {
        self.capabilities()
    }

    fn validate_config(&self, _config: &dyn crate::provider::ProviderConfig) -> VfsResult<()> {
        Ok(())
    }

    fn mount(
        &self,
        req: crate::provider::MountRequest<'_>,
    ) -> VfsResult<Arc<dyn FsSync>>;
}
