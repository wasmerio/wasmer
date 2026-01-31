use std::sync::Arc;

use vfs_core::provider::{config_downcast_ref, FsProviderCapabilities, MountRequest, ProviderConfig};
use vfs_core::{VfsError, VfsErrorKind, VfsResult};

use crate::config::HostFsConfig;
use crate::fs::HostFs;
use crate::platform;

#[derive(Clone, Debug)]
pub struct HostFsProvider;

impl vfs_core::traits_sync::FsProviderSync for HostFsProvider {
    fn name(&self) -> &'static str {
        "host"
    }

    fn capabilities(&self) -> FsProviderCapabilities {
        platform::provider_capabilities()
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        let Some(cfg) = config_downcast_ref::<HostFsConfig>(config) else {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "host.validate_config.type",
            ));
        };
        cfg.validate()
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsSync>> {
        let Some(cfg) = config_downcast_ref::<HostFsConfig>(req.config) else {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "host.mount.config_type",
            ));
        };
        Ok(Arc::new(HostFs::new(cfg.clone(), req.flags)?))
    }
}
