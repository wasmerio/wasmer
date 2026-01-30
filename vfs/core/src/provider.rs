//! Filesystem provider abstraction and capability flags.
//!
//! Terminology (Linux-like):
//! - [`FsProvider`]: filesystem type/driver (creates filesystem instances).
//! - [`Fs`]: a mounted filesystem instance (superblock-like).
//! - `Mount`: the binding of an [`Fs`] into the VFS namespace (implemented later in `mount.rs`).

use crate::path_types::{VfsPath, VfsPathBuf};
use crate::{Fs, VfsResult};
use bitflags::bitflags;
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct FsProviderCapabilities: u64 {
        const WATCH = 1 << 0;
        const HARDLINK = 1 << 1;
        const SYMLINK = 1 << 2;
        const RENAME_ATOMIC = 1 << 3;
        const ATOMIC_RENAME = 1 << 3;
        const SPARSE = 1 << 4;
        const XATTR = 1 << 5;
        const FILE_LOCKS = 1 << 6;
        const ATOMIC_O_TMPFILE = 1 << 7;
        const O_TMPFILE = 1 << 7;
        const CASE_SENSITIVE = 1 << 8;
        const CASE_PRESERVING = 1 << 9;
        const UNIX_PERMISSIONS = 1 << 10;
        const UTIMENS = 1 << 11;
        const STABLE_INODES = 1 << 12;
        const SEEK = 1 << 13;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct MountFlags: u64 {
        const READ_ONLY = 1 << 0;
        const NO_EXEC = 1 << 1;
        const NO_SUID = 1 << 2;
        const NODEV = 1 << 3;
        const NO_DEV = 1 << 3;
    }
}

/// Registry-visible provider name (e.g. "mem", "host", "overlay").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderName(pub Cow<'static, str>);

/// Optional filesystem instance id (used for logging or watch routing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FsInstanceId(pub u64);

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

pub type ProviderConfigBox = Box<dyn ProviderConfig>;

pub fn config_downcast_ref<T: 'static>(cfg: &dyn ProviderConfig) -> Option<&T> {
    cfg.as_any().downcast_ref::<T>()
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

    fn provider_capabilities(&self) -> FsProviderCapabilities {
        self.capabilities()
    }

    fn validate_config(&self, _config: &dyn ProviderConfig) -> VfsResult<()> {
        Ok(())
    }

    fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>>;
}

pub trait VfsRuntime: Send + Sync {
    fn spawn_blocking<F, R>(&self, f: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
        Self: Sized;

    fn block_on<F: Future>(&self, fut: F) -> F::Output
    where
        Self: Sized;
}

pub struct AsyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

pub struct SyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

/// Convenience config wrapper for providers that expect to receive a [`VfsPathBuf`].
#[derive(Debug, Clone)]
pub struct PathConfig {
    pub path: VfsPathBuf,
}

pub use crate::provider_registry::FsProviderRegistry;
