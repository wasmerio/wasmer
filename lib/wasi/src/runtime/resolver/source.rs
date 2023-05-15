use std::fmt::Debug;

use anyhow::Error;
use url::Url;

use crate::runtime::resolver::{PackageSpecifier, Summary};

/// Something that packages can be downloaded from.
#[async_trait::async_trait]
pub trait Source: Debug {
    /// An ID that describes this source.
    fn id(&self) -> SourceId;

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
    /// [`Summaries`][Summary] will be treated as the canonical source for
    /// that [`Dependency`][dep] and no further [`Source`]s will be queried.
    ///
    /// [dep]: crate::runtime::resolver::Dependency
    /// [reg]: crate::runtime::resolver::Registry
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error>;
}

#[async_trait::async_trait]
impl<D, S> Source for D
where
    D: std::ops::Deref<Target = S> + Debug + Send + Sync,
    S: Source + Send + Sync + 'static,
{
    fn id(&self) -> SourceId {
        (**self).id()
    }

    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        (**self).query(package).await
    }
}

/// The type of [`Source`] a [`SourceId`] corresponds to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// The path to a `*.webc` package on the file system.
    Path,
    /// The URL for a `*.webc` package on the internet.
    Url,
    /// The WAPM registry.
    Registry,
    /// A local directory containing packages laid out in a well-known
    /// format.
    LocalRegistry,
}

/// An ID associated with a [`Source`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceId {
    kind: SourceKind,
    url: Url,
}

impl SourceId {
    pub fn new(kind: SourceKind, url: Url) -> Self {
        SourceId { kind, url }
    }

    pub fn kind(&self) -> &SourceKind {
        &self.kind
    }

    pub fn url(&self) -> &Url {
        &self.url
    }
}
