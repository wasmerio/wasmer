use std::fmt::Debug;

use anyhow::Error;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary};

/// A collection of [`Source`][source]s.
///
/// [source]: crate::runtime::resolver::Source
#[async_trait::async_trait]
pub trait Registry: Send + Sync + Debug {
    async fn query(&self, pkg: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error>;

    /// Run [`Registry::query()`] and get the [`Summary`] for the latest
    /// version.
    async fn latest(&self, pkg: &PackageSpecifier) -> Result<PackageSummary, Error> {
        let candidates = self.query(pkg).await?;
        candidates
            .into_iter()
            .max_by(|left, right| left.pkg.version.cmp(&right.pkg.version))
            .ok_or_else(|| Error::msg("Couldn't find a package version satisfying that constraint"))
    }
}

#[async_trait::async_trait]
impl<D, R> Registry for D
where
    D: std::ops::Deref<Target = R> + Debug + Send + Sync,
    R: Registry + Send + Sync + ?Sized + 'static,
{
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error> {
        (**self).query(package).await
    }
}
