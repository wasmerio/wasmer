use std::{fmt::Debug, ops::Deref};

use anyhow::Error;
use webc::Container;

use crate::{
    bin_factory::BinaryPackage,
    runtime::resolver::{PackageSummary, Resolution},
};

pub fn to_module_hash(value: webc::metadata::AtomSignature) -> wasmer_types::ModuleHash {
    match value {
        webc::metadata::AtomSignature::Sha256(bytes) => wasmer_types::ModuleHash::Sha256(bytes),
    }
}

#[async_trait::async_trait]
pub trait PackageLoader: Send + Sync + Debug {
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error>;

    /// Load a resolved package into memory so it can be executed.
    ///
    /// A good default implementation is to just call
    /// [`load_package_tree()`][super::load_package_tree()].
    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error>;
}

#[async_trait::async_trait]
impl<D, P> PackageLoader for D
where
    D: Deref<Target = P> + Debug + Send + Sync,
    P: PackageLoader + ?Sized + 'static,
{
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error> {
        (**self).load(summary).await
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        (**self)
            .load_package_tree(root, resolution, root_is_local_dir)
            .await
    }
}
