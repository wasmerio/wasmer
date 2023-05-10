use std::{collections::HashMap, path::PathBuf};

use anyhow::Error;
use webc::compat::Container;

use crate::{
    bin_factory::BinaryPackage,
    runtime::resolver::{
        DependencyGraph, MultiSourceRegistry, PackageId, PackageResolver, PackageSpecifier,
        RootPackage,
    },
};

/// The builtin [`PackageResolver`].
#[derive(Debug, Clone)]
pub struct BuiltinResolver {
    _wasmer_home: PathBuf,
    registry: MultiSourceRegistry,
}

impl BuiltinResolver {
    pub fn new(wasmer_home: impl Into<PathBuf>) -> Self {
        BuiltinResolver {
            _wasmer_home: wasmer_home.into(),
            registry: MultiSourceRegistry::new(),
        }
    }

    async fn resolve(&self, root: RootPackage) -> Result<BinaryPackage, Error> {
        let (pkg, graph) = crate::runtime::resolver::resolve(&root, &self.registry).await?;
        let packages = self.download_packages(&graph).await?;
        crate::runtime::resolver::reconstitute(&pkg, &graph, &packages)
    }

    async fn download_packages(
        &self,
        _graph: &DependencyGraph,
    ) -> Result<HashMap<PackageId, Container>, Error> {
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
