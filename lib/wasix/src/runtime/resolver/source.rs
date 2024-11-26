use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

use wasmer_config::package::{PackageIdent, PackageSource};

use crate::runtime::resolver::PackageSummary;

/// Something that packages can be downloaded from.
#[async_trait::async_trait]
pub trait Source: Sync + Debug {
    /// Ask this source which packages would satisfy a particular
    /// [`Dependency`][dep] constraint.
    ///
    /// # Assumptions
    ///
    /// If this method returns a successful result, it is guaranteed that there
    /// will be at least one [`PackageSummary`], otherwise implementations
    /// should return [`QueryError::NotFound`] or [`QueryError::NoMatches`].
    ///
    /// [dep]: crate::runtime::resolver::Dependency
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError>;

    /// Run [`Source::query()`] and get the [`PackageSummary`] for the latest
    /// version.
    async fn latest(&self, pkg: &PackageSource) -> Result<PackageSummary, QueryError> {
        let candidates = self.query(pkg).await?;

        match pkg {
            PackageSource::Ident(PackageIdent::Named(_)) => candidates
                .into_iter()
                .max_by(|left, right| {
                    let left_version = left.pkg.id.as_named().map(|x| &x.version);
                    let right_version = right.pkg.id.as_named().map(|x| &x.version);

                    left_version.cmp(&right_version)
                })
                .ok_or(QueryError::NoMatches {
                    query: pkg.clone(),
                    archived_versions: Vec::new(),
                }),
            _ => candidates
                .into_iter()
                .next()
                .ok_or_else(|| QueryError::NotFound { query: pkg.clone() }),
        }
    }
}

#[async_trait::async_trait]
impl<D, S> Source for D
where
    D: std::ops::Deref<Target = S> + Debug + Send + Sync,
    S: Source + ?Sized + Send + Sync + 'static,
{
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        (**self).query(package).await
    }
}

#[derive(Clone, Debug)]
pub enum QueryError {
    Unsupported {
        query: PackageSource,
    },
    NotFound {
        query: PackageSource,
    },
    NoMatches {
        query: PackageSource,
        archived_versions: Vec<semver::Version>,
    },
    Timeout {
        query: PackageSource,
    },
    Other {
        query: PackageSource,
        // Arc to make it cloneable
        // Cloning is important for some use-cases.
        error: Arc<anyhow::Error>,
    },
}

impl QueryError {
    pub fn query(&self) -> &PackageSource {
        match self {
            Self::Unsupported { query }
            | Self::NotFound { query }
            | Self::NoMatches { query, .. }
            | Self::Timeout { query }
            | Self::Other { query, .. } => query,
        }
    }

    pub fn new_other(err: anyhow::Error, query: &PackageSource) -> Self {
        Self::Other {
            query: query.clone(),
            error: Arc::new(err),
        }
    }
}

impl Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to query package '{}': ", self.query())?;

        match self {
            Self::Unsupported { .. } => f.write_str("unsupported package specifier"),
            Self::NotFound { .. } => f.write_str("not found"),
            Self::Timeout { .. } => f.write_str("timeout"),
            Self::NoMatches {
                query: _,
                archived_versions,
            } => match archived_versions.as_slice() {
                [] => f.write_str(
                    "the package was found, but no published versions matched the constraint",
                ),
                [version] => write!(
                    f,
                    "the only version satisfying the constraint, {version}, is archived"
                ),
                [first, rest @ ..] => {
                    let num_others = rest.len();
                    write!(
                        f,
                        "unable to satisfy the request - version {first}, and {num_others} are all archived"
                    )
                }
            },
            Self::Other { error: e, query: _ } => Display::fmt(e, f),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other { error, query: _ } => Some(&***error),
            Self::Unsupported { .. }
            | Self::NotFound { .. }
            | Self::NoMatches { .. }
            | Self::Timeout { .. } => None,
        }
    }
}
