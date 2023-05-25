use std::fmt::Debug;

use anyhow::Error;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary};

/// Something that packages can be downloaded from.
#[async_trait::async_trait]
pub trait Source: Sync + Debug {
    /// Ask this source which packages would satisfy a particular
    /// [`Dependency`][dep] constraint.
    ///
    /// # Assumptions
    ///
    /// It is not an error if there are no package versions that may satisfy
    /// the dependency, even if the [`Source`] doesn't know of a package
    /// with that name.
    ///
    /// [dep]: crate::runtime::resolver::Dependency
    /// [reg]: crate::runtime::resolver::Registry
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error>;

    /// Run [`Source::query()`] and get the [`PackageSummary`] for the latest
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
impl<D, S> Source for D
where
    D: std::ops::Deref<Target = S> + Debug + Send + Sync,
    S: Source + ?Sized + Send + Sync + 'static,
{
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error> {
        (**self).query(package).await
    }
}
