use std::sync::Arc;

use vfs_core::provider::{
    FsProviderCapabilities, MountRequest, ProviderConfig, config_downcast_ref,
};
use vfs_core::traits_sync::FsProviderSync;
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

use crate::config::MemFsConfig;
use crate::fs::MemFs;

#[derive(Debug, Clone, Copy)]
pub struct MemFsProvider;

impl FsProviderSync for MemFsProvider {
    fn name(&self) -> &'static str {
        "mem"
    }

    fn capabilities(&self) -> FsProviderCapabilities {
        FsProviderCapabilities::SYMLINK
            | FsProviderCapabilities::HARDLINK
            | FsProviderCapabilities::UNIX_PERMISSIONS
            | FsProviderCapabilities::UTIMENS
            | FsProviderCapabilities::STABLE_INODES
            | FsProviderCapabilities::CASE_SENSITIVE
            | FsProviderCapabilities::CASE_PRESERVING
            | FsProviderCapabilities::SEEK
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        config_downcast_ref::<MemFsConfig>(config)
            .map(|_| ())
            .ok_or(VfsError::new(VfsErrorKind::InvalidInput, "memfs.config"))
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsSync>> {
        let config = config_downcast_ref::<MemFsConfig>(req.config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "memfs.mount.config",
        ))?;
        if matches!(config.max_inodes, Some(0)) {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "memfs.mount.max_inodes",
            ));
        }
        let fs = MemFs::new_with(config.clone(), req.flags);
        Ok(Arc::new(fs))
    }
}
