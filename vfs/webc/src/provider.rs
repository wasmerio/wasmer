
use std::sync::Arc;

use vfs_core::provider::{
    FsProviderCapabilities, MountFlags, MountRequest, ProviderConfig, config_downcast_ref,
};
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

use crate::config::WebcFsConfig;
use crate::fs::WebcFs;

#[derive(Debug, Clone)]
pub struct WebcFsProvider;

impl vfs_core::traits_sync::FsProviderSync for WebcFsProvider {
    fn name(&self) -> &'static str {
        "webc"
    }

    fn capabilities(&self) -> FsProviderCapabilities {
        FsProviderCapabilities::empty()
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        config_downcast_ref::<WebcFsConfig>(config)
            .map(|_| ())
            .ok_or(VfsError::new(VfsErrorKind::InvalidInput, "webc.config"))
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsSync>> {
        let cfg = config_downcast_ref::<WebcFsConfig>(req.config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "webc.mount.config",
        ))?;

        let _flags = req.flags | MountFlags::READ_ONLY;
        let fs = WebcFs::new(cfg.volume.clone(), cfg.root.clone());
        Ok(Arc::new(fs))
    }
}
