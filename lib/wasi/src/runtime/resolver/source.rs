use std::fmt::Debug;

use anyhow::Error;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary};

/// Something that packages can be downloaded from.
#[async_trait::async_trait]
pub trait Source: Debug {
    /// Ask this source which packages would satisfy a particular
    /// [`Dependency`][dep] constraint.
    ///
    /// # Assumptions
    ///
    /// It is not an error if there are no package versions that may satisfy
    /// the dependency, even if the [`Source`] doesn't know of a package
    /// with that name.
    ///
    /// A [`Registry`][reg] will typically have a list of [`Source`]s that are
    /// queried in order. The first [`Source`] to return one or more
    /// [`Summaries`][PackageSummary] will be treated as the canonical source
    /// for that [`Dependency`][dep] and no further [`Source`]s will be queried.
    ///
    /// [dep]: crate::runtime::resolver::Dependency
    /// [reg]: crate::runtime::resolver::Registry
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error>;
}

#[async_trait::async_trait]
impl<D, S> Source for D
where
    D: std::ops::Deref<Target = S> + Debug + Send + Sync,
    S: Source + Send + Sync + 'static,
{
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error> {
        (**self).query(package).await
    }
}
