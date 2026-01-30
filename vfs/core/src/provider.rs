//! Filesystem provider abstraction and capability flags.
//!
//! Terminology (Linux-like):
//! - [`FsProvider`]: filesystem type/driver (creates filesystem instances).
//! - [`Fs`]: a mounted filesystem instance (superblock-like).
//! - `Mount`: the binding of an [`Fs`] into the VFS namespace (implemented later in `mount.rs`).

use crate::path::{VfsPath, VfsPathBuf};
use crate::{Fs, VfsError, VfsResult};
use bitflags::bitflags;
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct FsProviderCapabilities: u64 {
        const WATCH = 1 << 0;
        const HARDLINK = 1 << 1;
        const SYMLINK = 1 << 2;
        const RENAME_ATOMIC = 1 << 3;
        const SPARSE = 1 << 4;
        const XATTR = 1 << 5;
        const FILE_LOCKS = 1 << 6;
        const ATOMIC_O_TMPFILE = 1 << 7;
        const CASE_SENSITIVE = 1 << 8;
        const CASE_PRESERVING = 1 << 9;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct MountFlags: u64 {
        const READ_ONLY = 1 << 0;
        const NO_EXEC = 1 << 1;
        const NO_SUID = 1 << 2;
        const NODEV = 1 << 3;
    }
}

/// Provider-specific configuration object for mount.
///
/// This is intentionally type-erased so a registry can store heterogeneous providers.
/// Providers can downcast to their concrete config type via [`ProviderConfig::as_any`].
pub trait ProviderConfig: Send + Sync + fmt::Debug + 'static {
    fn as_any(&self) -> &dyn Any;
}

impl<T> ProviderConfig for T
where
    T: Any + Send + Sync + fmt::Debug + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
pub struct MountRequest<'a> {
    /// Where this mount will be attached in the VFS namespace.
    ///
    /// This is VFS-level input (used for validation/logging); it does not imply the backend
    /// operates on global absolute paths.
    pub target_path: &'a VfsPath,
    pub flags: MountFlags,
    pub config: &'a dyn ProviderConfig,
}

pub trait FsProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> FsProviderCapabilities;

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>>;
}

#[derive(Default)]
pub struct FsProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn FsProvider>>>,
}

impl FsProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_provider(
        &self,
        name: impl Into<String>,
        provider: Arc<dyn FsProvider>,
    ) -> VfsResult<()> {
        let name = name.into();
        let mut providers = self
            .providers
            .write()
            .map_err(|_| VfsError::message("provider registry lock poisoned"))?;

        if providers.contains_key(&name) {
            return Err(VfsError::AlreadyExists);
        }

        providers.insert(name, provider);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn FsProvider>> {
        self.providers.read().ok()?.get(name).cloned()
    }

    pub fn list_names(&self) -> Vec<String> {
        let Ok(providers) = self.providers.read() else {
            return Vec::new();
        };
        let mut names: Vec<String> = providers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Create an [`Fs`] instance using a registered provider.
    ///
    /// Attaching the resulting filesystem into a mount table is handled by the mount layer
    /// (Phase 3).
    pub fn mount_with_provider(
        &self,
        provider_name: &str,
        config: &dyn ProviderConfig,
        target_path: &VfsPath,
        flags: MountFlags,
    ) -> VfsResult<Arc<dyn Fs>> {
        let provider = self.get(provider_name).ok_or(VfsError::NotFound)?;

        provider.mount(MountRequest {
            target_path,
            flags,
            config,
        })
    }
}

/// Convenience config wrapper for providers that expect to receive a [`VfsPathBuf`].
#[derive(Debug, Clone)]
pub struct PathConfig {
    pub path: VfsPathBuf,
}
