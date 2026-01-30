//! Filesystem provider registry.

use crate::path_types::VfsPath;
use crate::provider::{
    FsProvider, FsProviderCapabilities, MountFlags, MountRequest, ProviderConfig,
};
use crate::{Fs, VfsError, VfsErrorKind, VfsResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderInfo {
    pub name: String,
    pub capabilities: FsProviderCapabilities,
}

fn normalize_provider_name(input: &str) -> VfsResult<String> {
    let trimmed = input.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return Err(VfsError::new(
            VfsErrorKind::InvalidInput,
            "provider_registry.name.empty",
        ));
    }

    let mut normalized = String::with_capacity(trimmed.len());
    for byte in trimmed.as_bytes() {
        if !byte.is_ascii() {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "provider_registry.name.invalid_char",
            ));
        }
        let lower = byte.to_ascii_lowercase();
        let allowed = matches!(lower, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-');
        if !allowed {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "provider_registry.name.invalid_char",
            ));
        }
        normalized.push(lower as char);
    }

    Ok(normalized)
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
        name: impl AsRef<str>,
        provider: Arc<dyn FsProvider>,
    ) -> VfsResult<()> {
        let name = normalize_provider_name(name.as_ref())?;
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

    pub fn get(&self, name: &str) -> VfsResult<Option<Arc<dyn FsProvider>>> {
        let name = normalize_provider_name(name)?;
        let providers = self
            .providers
            .read()
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
        Ok(providers.get(&name).cloned())
    }

    pub fn list_names(&self) -> VfsResult<Vec<String>> {
        let providers = self
            .providers
            .read()
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
        let mut names: Vec<String> = providers.keys().cloned().collect();
        names.sort();
        Ok(names)
    }

    pub fn provider_capabilities(&self, name: &str) -> VfsResult<FsProviderCapabilities> {
        let name = normalize_provider_name(name)?;
        let provider = {
            let providers = self
                .providers
                .read()
                .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
            providers.get(&name).cloned().ok_or(VfsError::new(
                VfsErrorKind::NotFound,
                "provider_registry.get",
            ))?
        };
        Ok(provider.provider_capabilities())
    }

    pub fn describe_provider(&self, name: &str) -> VfsResult<ProviderInfo> {
        let name = normalize_provider_name(name)?;
        let provider = {
            let providers = self
                .providers
                .read()
                .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
            providers.get(&name).cloned().ok_or(VfsError::new(
                VfsErrorKind::NotFound,
                "provider_registry.get",
            ))?
        };
        Ok(ProviderInfo {
            name,
            capabilities: provider.provider_capabilities(),
        })
    }

    pub fn list_providers(&self) -> VfsResult<Vec<ProviderInfo>> {
        let providers: Vec<(String, Arc<dyn FsProvider>)> = {
            let providers = self
                .providers
                .read()
                .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
            providers
                .iter()
                .map(|(name, provider)| (name.clone(), provider.clone()))
                .collect()
        };

        let mut infos: Vec<ProviderInfo> = providers
            .into_iter()
            .map(|(name, provider)| ProviderInfo {
                name,
                capabilities: provider.provider_capabilities(),
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(infos)
    }

    /// Create an [`Fs`] instance using a registered provider.
    ///
    /// Attaching the resulting filesystem into a mount table is handled by the mount layer
    /// (Phase 3).
    pub fn create_fs(&self, provider_name: &str, req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>> {
        let provider_name = normalize_provider_name(provider_name)?;
        let provider = {
            let providers = self
                .providers
                .read()
                .map_err(|_| VfsError::new(VfsErrorKind::Internal, "provider_registry.lock"))?;
            providers.get(&provider_name).cloned().ok_or(VfsError::new(
                VfsErrorKind::NotFound,
                "provider_registry.get",
            ))?
        };

        provider.validate_config(req.config)?;
        provider.mount(req)
    }

    pub fn create_fs_with_provider(
        &self,
        provider_name: &str,
        config: &dyn ProviderConfig,
        target_path: &VfsPath,
        flags: MountFlags,
    ) -> VfsResult<Arc<dyn Fs>> {
        self.create_fs(
            provider_name,
            MountRequest {
                target_path,
                flags,
                config,
            },
        )
    }

    pub fn mount_with_provider(
        &self,
        provider_name: &str,
        config: &dyn ProviderConfig,
        target_path: &VfsPath,
        flags: MountFlags,
    ) -> VfsResult<Arc<dyn Fs>> {
        self.create_fs_with_provider(provider_name, config, target_path, flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{
        CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
        SetMetadata, UnlinkOptions,
    };
    use crate::path_types::VfsPathBuf;
    use crate::provider::config_downcast_ref;
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
            BackendInodeId::new(1).expect("non-zero inode")
        }

        fn file_type(&self) -> VfsFileType {
            VfsFileType::Directory
        }

        fn metadata(&self) -> VfsResult<crate::VfsMetadata> {
            Ok(crate::VfsMetadata {
                inode: VfsInodeId {
                    mount: MountId::from_index(0),
                    backend: BackendInodeId::new(1).expect("non-zero inode"),
                },
                file_type: VfsFileType::Directory,
                mode: crate::VfsFileMode(0),
                uid: 0,
                gid: 0,
                nlink: 1,
                size: 0,
                atime: crate::VfsTimespec { secs: 0, nanos: 0 },
                mtime: crate::VfsTimespec { secs: 0, nanos: 0 },
                ctime: crate::VfsTimespec { secs: 0, nanos: 0 },
                rdev_major: 0,
                rdev_minor: 0,
            })
        }

        fn set_metadata(&self, _set: SetMetadata) -> VfsResult<()> {
            self.unsupported("dummy.set_metadata")
        }

        fn lookup(&self, _name: &crate::VfsName) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("dummy.lookup")
        }

        fn create_file(
            &self,
            _name: &crate::VfsName,
            _opts: CreateFile,
        ) -> VfsResult<Arc<dyn FsNode>> {
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
        caps: FsProviderCapabilities,
    }

    impl DummyProvider {
        fn new(caps: FsProviderCapabilities) -> Self {
            Self {
                mounts: AtomicUsize::new(0),
                caps,
            }
        }
    }

    impl FsProvider for DummyProvider {
        fn name(&self) -> &'static str {
            "dummy"
        }

        fn capabilities(&self) -> FsProviderCapabilities {
            self.caps
        }

        fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()> {
            if config_downcast_ref::<DummyConfig>(config).is_some() {
                Ok(())
            } else {
                Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "dummy.validate_config",
                ))
            }
        }

        fn mount(&self, _req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>> {
            self.mounts.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(DummyFs::new()))
        }
    }

    struct LockCheckProvider {
        registry: Arc<FsProviderRegistry>,
    }

    impl FsProvider for LockCheckProvider {
        fn name(&self) -> &'static str {
            "lockcheck"
        }

        fn capabilities(&self) -> FsProviderCapabilities {
            FsProviderCapabilities::empty()
        }

        fn mount(&self, _req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>> {
            assert!(self.registry.providers.try_read().is_ok());
            Ok(Arc::new(DummyFs::new()))
        }
    }

    #[test]
    fn register_and_get_provider() {
        let registry = FsProviderRegistry::new();
        registry
            .register(Arc::new(
                DummyProvider::new(FsProviderCapabilities::empty()),
            ))
            .expect("register should succeed");

        assert!(registry.get("dummy").expect("get should succeed").is_some());
    }

    #[test]
    fn duplicate_register_fails() {
        let registry = FsProviderRegistry::new();
        registry
            .register_provider(
                "dummy",
                Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
            )
            .expect("first register should succeed");

        let err = registry
            .register_provider(
                "dummy",
                Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
            )
            .expect_err("duplicate register should fail");
        assert_eq!(err.kind(), VfsErrorKind::AlreadyExists);
    }

    #[test]
    fn normalized_register_and_lookup() {
        let registry = FsProviderRegistry::new();
        registry
            .register_provider(
                "  HoSt  ",
                Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
            )
            .expect("register should succeed");

        assert!(registry.get("host").expect("get should succeed").is_some());
        assert!(registry.get("HOST").expect("get should succeed").is_some());
    }

    #[test]
    fn invalid_provider_names_rejected() {
        let registry = FsProviderRegistry::new();
        for name in ["", "   ", "host!", "☃"] {
            let err = registry
                .register_provider(
                    name,
                    Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
                )
                .expect_err("register should fail");
            assert_eq!(err.kind(), VfsErrorKind::InvalidInput);
        }

        let err = match registry.get("bad!") {
            Ok(_) => panic!("get should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::InvalidInput);
    }

    #[test]
    fn list_names_returns_sorted() {
        let registry = FsProviderRegistry::new();
        registry
            .register_provider(
                "b",
                Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
            )
            .expect("register should succeed");
        registry
            .register_provider(
                "a",
                Arc::new(DummyProvider::new(FsProviderCapabilities::empty())),
            )
            .expect("register should succeed");

        let names = registry.list_names().expect("list should succeed");
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn provider_capabilities_are_reported() {
        let registry = FsProviderRegistry::new();
        let caps = FsProviderCapabilities::SYMLINK | FsProviderCapabilities::UTIMENS;
        registry
            .register_provider("dummy", Arc::new(DummyProvider::new(caps)))
            .expect("register should succeed");

        let reported = registry
            .provider_capabilities("dummy")
            .expect("capabilities should succeed");
        assert_eq!(reported, caps);
    }

    #[test]
    fn list_providers_is_sorted() {
        let registry = FsProviderRegistry::new();
        registry
            .register_provider(
                "b",
                Arc::new(DummyProvider::new(FsProviderCapabilities::UTIMENS)),
            )
            .expect("register should succeed");
        registry
            .register_provider(
                "a",
                Arc::new(DummyProvider::new(FsProviderCapabilities::SYMLINK)),
            )
            .expect("register should succeed");

        let providers = registry.list_providers().expect("list should succeed");
        let names: Vec<String> = providers.iter().map(|info| info.name.clone()).collect();
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn mount_with_provider_calls_mount() {
        let registry = FsProviderRegistry::new();
        let provider = Arc::new(DummyProvider::new(FsProviderCapabilities::empty()));
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
        let provider = Arc::new(DummyProvider::new(FsProviderCapabilities::empty()));
        registry
            .register_provider("dummy", provider)
            .expect("register should succeed");

        let err = match registry.mount_with_provider(
            "dummy",
            &OtherConfig,
            VfsPath::new(b"/"),
            MountFlags::empty(),
        ) {
            Ok(_) => panic!("mount should fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::InvalidInput);
    }

    #[test]
    fn mount_does_not_hold_registry_lock() {
        let registry = Arc::new(FsProviderRegistry::new());
        registry
            .register_provider(
                "lockcheck",
                Arc::new(LockCheckProvider {
                    registry: registry.clone(),
                }),
            )
            .expect("register should succeed");

        registry
            .mount_with_provider(
                "lockcheck",
                &DummyConfig,
                VfsPath::new(b"/"),
                MountFlags::empty(),
            )
            .expect("mount should succeed");
    }
}
