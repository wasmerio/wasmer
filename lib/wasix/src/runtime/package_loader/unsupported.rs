use anyhow::Error;
use webc::Container;

use crate::{
    bin_factory::BinaryPackage,
    runtime::{
        package_loader::PackageLoader,
        resolver::{PackageSummary, Resolution},
    },
};

/// A [`PackageLoader`] implementation which will always error out.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct UnsupportedPackageLoader;

#[async_trait::async_trait]
impl PackageLoader for UnsupportedPackageLoader {
    async fn load(&self, _summary: &PackageSummary) -> Result<Container, Error> {
        Err(Error::new(Unsupported))
    }

    async fn load_package_tree(
        &self,
        _root: &Container,
        _resolution: &Resolution,
        _root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        Err(Error::new(Unsupported))
    }
}

#[derive(Debug, Copy, Clone, thiserror::Error)]
#[error("Loading of packages is not supported in this runtime (no PackageLoader configured)")]
struct Unsupported;
