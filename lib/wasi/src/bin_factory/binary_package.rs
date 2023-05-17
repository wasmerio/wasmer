use std::sync::Arc;

use derivative::*;
use once_cell::sync::OnceCell;
use semver::Version;
use virtual_fs::FileSystem;
use webc::{compat::SharedBytes, Container};

use crate::{
    runtime::{
        module_cache::ModuleHash,
        resolver::{PackageId, PackageInfo, PackageSpecifier, SourceId, SourceKind},
    },
    WasiRuntime,
};

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackageCommand {
    name: String,
    #[derivative(Debug = "ignore")]
    pub(crate) atom: SharedBytes,
    hash: OnceCell<ModuleHash>,
}

impl BinaryPackageCommand {
    pub fn new(name: String, atom: SharedBytes) -> Self {
        Self {
            name,
            atom,
            hash: OnceCell::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a reference to this [`BinaryPackageCommand`]'s atom.
    ///
    /// The address of the returned slice is guaranteed to be stable and live as
    /// long as the [`BinaryPackageCommand`].
    pub fn atom(&self) -> &[u8] {
        &self.atom
    }

    pub fn hash(&self) -> &ModuleHash {
        self.hash.get_or_init(|| ModuleHash::sha256(self.atom()))
    }
}

/// A WebAssembly package that has been loaded into memory.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: String,
    pub when_cached: Option<u128>,
    #[derivative(Debug = "ignore")]
    pub entry: Option<SharedBytes>,
    pub hash: OnceCell<ModuleHash>,
    pub webc_fs: Arc<dyn FileSystem + Send + Sync>,
    pub commands: Vec<BinaryPackageCommand>,
    pub uses: Vec<String>,
    pub version: Version,
    pub module_memory_footprint: u64,
    pub file_system_memory_footprint: u64,
}

impl BinaryPackage {
    /// Load a [`webc::Container`] and all its dependencies into a
    /// [`BinaryPackage`].
    pub async fn from_webc(
        container: &Container,
        rt: &dyn WasiRuntime,
    ) -> Result<Self, anyhow::Error> {
        let registry = rt.registry();
        let root = PackageInfo::from_manifest(container.manifest())?;
        let root_id = PackageId {
            package_name: root.name.clone(),
            version: root.version.clone(),
            source: SourceId::new(
                SourceKind::LocalRegistry,
                "http://localhost/".parse().unwrap(),
            ),
        };

        let resolution = crate::runtime::resolver::resolve(&root_id, &root, &*registry).await?;
        let pkg = rt
            .load_package_tree(container, &resolution)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    /// Load a [`BinaryPackage`] and all its dependencies from a registry.
    pub async fn from_specifier(
        specifier: &PackageSpecifier,
        runtime: &dyn WasiRuntime,
    ) -> Result<Self, anyhow::Error> {
        let registry = runtime.registry();
        let root_summary = registry.latest(specifier).await?;
        let root = runtime.package_loader().load(&root_summary).await?;
        let id = root_summary.package_id();

        let resolution =
            crate::runtime::resolver::resolve(&id, &root_summary.pkg, &registry).await?;
        let pkg = runtime
            .load_package_tree(&root, &resolution)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    pub fn hash(&self) -> ModuleHash {
        *self.hash.get_or_init(|| {
            if let Some(entry) = self.entry.as_ref() {
                ModuleHash::sha256(entry)
            } else {
                ModuleHash::sha256(self.package_name.as_bytes())
            }
        })
    }
}
