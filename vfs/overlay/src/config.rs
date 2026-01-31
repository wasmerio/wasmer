use std::sync::Arc;

use vfs_core::provider::{FsProvider, MountRequest, ProviderConfig, config_downcast_ref};
use vfs_core::{Fs, FsProviderRegistry, VfsError, VfsErrorKind, VfsResult};

use crate::fs::OverlayFs;

#[derive(Clone, Debug)]
pub struct OverlayOptions {
    pub max_copy_chunk_size: usize,
    pub deny_reserved_names: bool,
    pub lower_readonly: bool,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            max_copy_chunk_size: 128 * 1024,
            deny_reserved_names: true,
            lower_readonly: true,
        }
    }
}

pub struct OverlayBuilder {
    upper: Arc<dyn Fs>,
    lowers: Vec<Arc<dyn Fs>>,
    opts: OverlayOptions,
}

impl OverlayBuilder {
    pub fn new(upper: Arc<dyn Fs>, lowers: Vec<Arc<dyn Fs>>) -> Self {
        Self {
            upper,
            lowers,
            opts: OverlayOptions::default(),
        }
    }

    pub fn with_options(mut self, opts: OverlayOptions) -> Self {
        self.opts = opts;
        self
    }

    pub fn build(self) -> VfsResult<OverlayFs> {
        OverlayFs::new(self.upper, self.lowers, self.opts)
    }
}

#[derive(Debug)]
pub struct FsSpec {
    pub provider: String,
    pub config: Box<dyn ProviderConfig>,
}

#[derive(Debug)]
pub struct OverlayConfig {
    pub upper: FsSpec,
    pub lowers: Vec<FsSpec>,
    pub options: OverlayOptions,
}

pub struct OverlayProvider {
    registry: Arc<FsProviderRegistry>,
}

impl OverlayProvider {
    pub fn new(registry: Arc<FsProviderRegistry>) -> Self {
        Self { registry }
    }
}

impl FsProvider for OverlayProvider {
    fn name(&self) -> &'static str {
        "overlay"
    }

    fn capabilities(&self) -> vfs_core::provider::FsProviderCapabilities {
        vfs_core::provider::FsProviderCapabilities::empty()
    }

    fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
        let cfg = config_downcast_ref::<OverlayConfig>(config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "overlay.validate_config",
        ))?;
        if cfg.lowers.is_empty() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "overlay.validate_config.lowers",
            ));
        }
        if cfg.upper.provider.trim().is_empty() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "overlay.validate_config.upper_provider",
            ));
        }
        for lower in &cfg.lowers {
            if lower.provider.trim().is_empty() {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "overlay.validate_config.lower_provider",
                ));
            }
        }
        Ok(())
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>> {
        let cfg = config_downcast_ref::<OverlayConfig>(req.config).ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "overlay.mount.config",
        ))?;

        let upper = self.registry.create_fs_with_provider(
            &cfg.upper.provider,
            &*cfg.upper.config,
            req.target_path,
            req.flags,
        )?;
        let mut lowers = Vec::with_capacity(cfg.lowers.len());
        for layer in &cfg.lowers {
            let fs = self.registry.create_fs_with_provider(
                &layer.provider,
                &*layer.config,
                req.target_path,
                req.flags,
            )?;
            lowers.push(fs);
        }

        let overlay =
            OverlayFs::new_with_mount_flags(upper, lowers, cfg.options.clone(), req.flags)?;
        Ok(Arc::new(overlay))
    }
}
