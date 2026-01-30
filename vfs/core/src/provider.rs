//! Filesystem provider abstraction and capability flags.
//!
//! Terminology (Linux-like):
//! - [`FsProvider`]: filesystem type/driver (creates filesystem instances).
//! - [`Fs`]: a mounted filesystem instance (superblock-like).
//! - `Mount`: the binding of an [`Fs`] into the VFS namespace (implemented later in `mount.rs`).

use crate::path_types::{VfsPath, VfsPathBuf};
use crate::{Fs, VfsError, VfsErrorKind, VfsResult};
use bitflags::bitflags;
use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

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
        R: Send + 'static;

    fn block_on<F: Future>(&self, fut: F) -> F::Output;
}

pub struct AsyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

pub struct SyncAdapter<T> {
    pub inner: T,
    pub rt: Arc<dyn VfsRuntime>,
}

#[derive(Default)]
pub struct FsProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn FsProvider>>>,
}

impl FsProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, provider: Arc<dyn FsProvider>) -> VfsResult<()> {
        self.register_provider(provider.name(), provider)
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
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;

        if providers.contains_key(&name) {
            return Err(VfsError::new(
                VfsErrorKind::AlreadyExists,
                "provider_registry.register",
            ));
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
        let provider = self
            .get(provider_name)
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "provider_registry.get"))?;

        provider.validate_config(config)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{
        CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
        SetMetadata, UnlinkOptions,
    };
    use crate::{BackendInodeId, MountId, VfsCapabilities, VfsErrorKind, VfsFileType, VfsInodeId};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DummyConfig;

    #[derive(Debug)]
    struct OtherConfig;

    struct DummyNode;

    impl DummyNode {
        fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
            Err(VfsError::new(VfsErrorKind::NotSupported, op))
        }
    }

    impl FsNode for DummyNode {
        fn inode(&self) -> BackendInodeId {
            BackendInodeId(1)
        }

        fn file_type(&self) -> VfsFileType {
            VfsFileType::Directory
        }

        fn metadata(&self) -> VfsResult<crate::VfsMetadata> {
            Ok(crate::VfsMetadata {
                inode: VfsInodeId {
                    mount: MountId(0),
                    backend: BackendInodeId(1),
                },
                file_type: VfsFileType::Directory,
                mode: 0,
                uid: 0,
                gid: 0,
                nlink: 1,
                size: 0,
                atime: None,
                mtime: None,
                ctime: None,
                rdev: 0,
            })
        }

        fn set_metadata(&self, _set: SetMetadata) -> VfsResult<()> {
            self.unsupported("dummy.set_metadata")
        }

        fn lookup(&self, _name: &crate::VfsName) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("dummy.lookup")
        }

        fn create_file(&self, _name: &crate::VfsName, _opts: CreateFile) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("dummy.create_file")
        }

        fn mkdir(&self, _name: &crate::VfsName, _opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("dummy.mkdir")
        }

        fn unlink(&self, _name: &crate::VfsName, _opts: UnlinkOptions) -> VfsResult<()> {
            self.unsupported("dummy.unlink")
        }

        fn rmdir(&self, _name: &crate::VfsName) -> VfsResult<()> {
            self.unsupported("dummy.rmdir")
        }

        fn read_dir(&self, _cursor: Option<DirCursor>, _max: usize) -> VfsResult<ReadDirBatch> {
            self.unsupported("dummy.read_dir")
        }

        fn rename(
            &self,
            _old_name: &crate::VfsName,
            _new_parent: &dyn FsNode,
            _new_name: &crate::VfsName,
            _opts: RenameOptions,
        ) -> VfsResult<()> {
            self.unsupported("dummy.rename")
        }

        fn open(&self, _opts: crate::flags::OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
            self.unsupported("dummy.open")
        }

        fn link(&self, _existing: &dyn FsNode, _new_name: &crate::VfsName) -> VfsResult<()> {
            self.unsupported("dummy.link")
        }

        fn symlink(&self, _new_name: &crate::VfsName, _target: &VfsPath) -> VfsResult<()> {
            self.unsupported("dummy.symlink")
        }

        fn readlink(&self) -> VfsResult<VfsPathBuf> {
            self.unsupported("dummy.readlink")
        }
    }

    struct DummyFs {
        root: Arc<dyn FsNode>,
    }

    impl DummyFs {
        fn new() -> Self {
            Self {
                root: Arc::new(DummyNode),
            }
        }
    }

    impl Fs for DummyFs {
        fn provider_name(&self) -> &'static str {
            "dummy"
        }

        fn capabilities(&self) -> VfsCapabilities {
            VfsCapabilities::NONE
        }

        fn root(&self) -> Arc<dyn FsNode> {
            self.root.clone()
        }
    }

    struct DummyProvider {
        mounts: AtomicUsize,
    }

    impl DummyProvider {
        fn new() -> Self {
            Self {
                mounts: AtomicUsize::new(0),
            }
        }
    }

    impl FsProvider for DummyProvider {
        fn name(&self) -> &'static str {
            "dummy"
        }

        fn capabilities(&self) -> FsProviderCapabilities {
            FsProviderCapabilities::empty()
        }

        fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
            if config_downcast_ref::<DummyConfig>(config).is_some() {
                Ok(())
            } else {
                Err(VfsError::new(VfsErrorKind::InvalidInput, "dummy.validate_config"))
            }
        }

        fn mount(&self, _req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>> {
            self.mounts.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(DummyFs::new()))
        }
    }

    #[test]
    fn register_and_get_provider() {
        let registry = FsProviderRegistry::new();
        registry
            .register(Arc::new(DummyProvider::new()))
            .expect("register should succeed");

        assert!(registry.get("dummy").is_some());
    }

    #[test]
    fn duplicate_register_fails() {
        let registry = FsProviderRegistry::new();
        registry
            .register_provider("dummy", Arc::new(DummyProvider::new()))
            .expect("first register should succeed");

        let err = registry
            .register_provider("dummy", Arc::new(DummyProvider::new()))
            .expect_err("duplicate register should fail");
        assert_eq!(err.kind(), VfsErrorKind::AlreadyExists);
    }

    #[test]
    fn mount_with_provider_calls_mount() {
        let registry = FsProviderRegistry::new();
        let provider = Arc::new(DummyProvider::new());
        let config = DummyConfig;
        registry
            .register_provider("dummy", provider.clone())
            .expect("register should succeed");

        registry
            .mount_with_provider("dummy", &config, VfsPath::new(b"/"), MountFlags::empty())
            .expect("mount should succeed");

        assert_eq!(provider.mounts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn config_mismatch_produces_invalid_input() {
        let registry = FsProviderRegistry::new();
        let provider = Arc::new(DummyProvider::new());
        registry
            .register_provider("dummy", provider)
            .expect("register should succeed");

        let err = registry
            .mount_with_provider("dummy", &OtherConfig, VfsPath::new(b"/"), MountFlags::empty())
            .expect_err("mount should fail");
        assert_eq!(err.kind(), VfsErrorKind::InvalidInput);
    }
}
