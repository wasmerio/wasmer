use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Error;
use url::Url;
use webc::compat::Container;

use crate::{
    bin_factory::BinaryPackage,
    runtime::resolver::{
        DependencyGraph, MultiSourceRegistry, PackageId, PackageResolver, PackageSpecifier,
        RootPackage, Source, WapmSource,
    },
};

/// The builtin [`PackageResolver`] that is used by the `wasmer` CLI and
/// respects `$WASMER_HOME`.
#[derive(Debug, Clone)]
pub struct BuiltinResolver {
    _wasmer_home: PathBuf,
    registry: MultiSourceRegistry,
}

impl BuiltinResolver {
    pub fn new(wasmer_home: impl Into<PathBuf>, registry: MultiSourceRegistry) -> Self {
        BuiltinResolver {
            _wasmer_home: wasmer_home.into(),
            registry,
        }
    }

    /// Create a new [`BuiltinResolver`] based on `$WASMER_HOME` and the global
    /// Wasmer config.
    pub fn from_env() -> Result<Self, Error> {
        let wasmer_home = discover_wasmer_home()?;
        let active_registry = active_registry(&wasmer_home)?;
        let source = WapmSource::new(active_registry);
        BuiltinResolver::from_env_with_sources(vec![Arc::new(source)])
    }

    /// Create a new [`BuiltinResolver`] based on `$WASMER_HOME` that will use
    /// the provided [`Source`]s when doing queries.
    pub fn from_env_with_sources(
        sources: Vec<Arc<dyn Source + Send + Sync>>,
    ) -> Result<Self, Error> {
        let wasmer_home = discover_wasmer_home()?;

        let mut registry = MultiSourceRegistry::new();
        for source in sources {
            registry.add_shared_source(source);
        }

        Ok(BuiltinResolver::new(wasmer_home, registry))
    }

    async fn resolve(&self, root: RootPackage) -> Result<BinaryPackage, Error> {
        let (pkg, graph) = crate::runtime::resolver::resolve(&root, &self.registry).await?;
        let packages = self.fetch_packages(&graph).await?;
        crate::runtime::resolver::reconstitute(&pkg, &graph, &packages)
    }

    async fn fetch_packages(
        &self,
        _graph: &DependencyGraph,
    ) -> Result<HashMap<PackageId, Container>, Error> {
        // Note: we can speed this up quite a bit by caching things to
        // `$WASMER_HOME/checkouts/` and in memory, and using the SHA-256 hash
        // attached to every package's `Summary`. Otherwise, the `Summary`
        // includes a URL we can download from.
        todo!();
    }
}

#[async_trait::async_trait]
impl PackageResolver for BuiltinResolver {
    async fn load_package(&self, pkg: &PackageSpecifier) -> Result<BinaryPackage, Error> {
        let root = RootPackage::from_registry(pkg, &self.registry).await?;
        self.resolve(root).await
    }

    async fn load_webc(&self, webc: &Container) -> Result<BinaryPackage, Error> {
        let root = RootPackage::from_webc_metadata(webc.manifest());
        self.resolve(root).await
    }
}

fn discover_wasmer_home() -> Result<PathBuf, Error> {
    todo!();
}

fn active_registry(_wasmer_home: &Path) -> Result<Url, Error> {
    todo!();
}
