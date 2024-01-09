use std::fmt::{Debug, Display};

use crate::runtime::resolver::{PackageSpecifier, PackageSummary};

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
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError>;

    /// Run [`Source::query()`] and get the [`PackageSummary`] for the latest
    /// version.
    async fn latest(&self, pkg: &PackageSpecifier) -> Result<PackageSummary, QueryError> {
        let candidates = self.query(pkg).await?;
        candidates
            .into_iter()
            .max_by(|left, right| left.pkg.version.cmp(&right.pkg.version))
            .ok_or(QueryError::NoMatches {
                archived_versions: Vec::new(),
            })
    }
}

#[async_trait::async_trait]
impl<D, S> Source for D
where
    D: std::ops::Deref<Target = S> + Debug + Send + Sync,
    S: Source + ?Sized + Send + Sync + 'static,
{
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError> {
        (**self).query(package).await
    }
}

#[derive(Debug)]
pub enum QueryError {
    Unsupported,
    NotFound,
    NoMatches {
        archived_versions: Vec<semver::Version>,
    },
    Timeout,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for QueryError {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(value)
    }
}

impl Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("This type of package specifier isn't supported"),
            Self::NotFound => f.write_str("Not found"),
            Self::Timeout => f.write_str("Timed out"),
            Self::NoMatches { archived_versions } => match archived_versions.as_slice() {
                [] => f.write_str(
                    "The package was found, but no published versions matched the constraint",
                ),
                [version] => write!(
                    f,
                    "The only version satisfying the constraint, {version}, is archived"
                ),
                [first, rest @ ..] => {
                    let num_others = rest.len();
                    write!(
                        f,
                        "Unable to satisfy the request. Version {first}, and {num_others} are all archived"
                    )
                }
            },
            Self::Other(e) => Display::fmt(e, f),
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(&**e),
            Self::Unsupported | Self::NotFound | Self::NoMatches { .. } | Self::Timeout => None,
        }
    }
}
