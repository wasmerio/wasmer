use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    ops::Deref,
    path::PathBuf,
    str::FromStr,
};

use anyhow::{Context, Error};
use semver::{Version, VersionReq};
use url::Url;
use webc::{compat::Container, metadata::Manifest};

#[async_trait::async_trait]
pub trait PackageLoader: Debug {
    async fn load(&self, summary: &Summary) -> Result<Container, Error>;
}

#[async_trait::async_trait]
impl<D, P> PackageLoader for D
where
    D: Deref<Target = P> + Debug + Send + Sync,
    P: PackageLoader + Send + Sync + ?Sized + 'static,
{
    async fn load(&self, summary: &Summary) -> Result<Container, Error> {
        (**self).load(summary).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    package: ResolvedPackage,
    graph: DependencyGraph,
}

/// A reference to *some* package somewhere that the user wants to run.
///
/// # Security Considerations
///
/// The [`PackageSpecifier::Path`] variant doesn't specify which filesystem a
/// [`Source`] will eventually query. Consumers of [`PackageSpecifier`] should
/// be wary of sandbox escapes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageSpecifier {
    Registry {
        full_name: String,
        version: VersionReq,
    },
    Url(Url),
    /// A `*.webc` file on disk.
    Path(PathBuf),
}

impl FromStr for PackageSpecifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // TODO: Replace this with something more rigorous that can also handle
        // the locator field
        let (full_name, version) = match s.split_once('@') {
            Some((n, v)) => (n, v),
            None => (s, "*"),
        };

        let invalid_character = full_name
            .char_indices()
            .find(|(_, c)| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.'| '-'|'_' | '/'));
        if let Some((index, c)) = invalid_character {
            anyhow::bail!("Invalid character, {c:?}, at offset {index}");
        }

        let version = version
            .parse()
            .with_context(|| format!("Invalid version number, \"{version}\""))?;

        Ok(PackageSpecifier::Registry {
            full_name: full_name.to_string(),
            version,
        })
    }
}

/// A collection of [`Source`]s.
#[async_trait::async_trait]
pub trait Registry: Debug {
    async fn query(&self, pkg: &PackageSpecifier) -> Result<Vec<Summary>, Error>;
}

#[async_trait::async_trait]
impl<D, R> Registry for D
where
    D: std::ops::Deref<Target = R> + Debug + Send + Sync,
    R: Registry + Send + Sync + ?Sized + 'static,
{
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        (**self).query(package).await
    }
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

/// Something that packages can be downloaded from.
#[async_trait::async_trait]
pub trait Source: Debug {
    /// An ID that describes this source.
    fn id(&self) -> SourceId;

    /// Ask this source which packages would satisfy a particular
    /// [`Dependency`] constraint.
    ///
    /// # Assumptions
    ///
    /// It is not an error if there are no package versions that may satisfy
    /// the dependency, even if the [`Source`] doesn't know of a package
    /// with that name.
    ///
    /// A [`Registry`] will typically have a list of [`Source`]s that are
    /// queried in order. The first [`Source`] to return one or more
    /// [`Summaries`][Summary] will be treated as the canonical source for
    /// that [`Dependency`] and no further [`Source`]s will be queried.
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

/// A dependency constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The package's actual name.
    package_name: String,
    /// The name that will be used to refer to this package.
    alias: Option<String>,
    /// Which versions of the package are requested?
    version: VersionReq,
}

impl Dependency {
    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    pub fn version(&self) -> &VersionReq {
        &self.version
    }
}

/// Some metadata a [`Source`] can provide about a package without needing
/// to download the entire `*.webc` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The package's full name (i.e. `wasmer/wapm2pirita`).
    pub package_name: String,
    /// The package version.
    pub version: Version,
    /// A URL that can be used to download the `*.webc` file.
    pub webc: Url,
    /// A SHA-256 checksum for the `*.webc` file.
    pub webc_sha256: [u8; 32],
    /// Any dependencies this package may have.
    pub dependencies: Vec<Dependency>,
    /// Commands this package exposes to the outside world.
    pub commands: Vec<Command>,
    /// The [`Source`] this [`Summary`] came from.
    pub source: SourceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    // atom: ItemLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemLocation {
    /// Something within the current package.
    CurrentPackage {
        /// The item's name.
        name: String,
    },
    /// Something that is part of a dependency.
    Dependency {
        /// The name used to refer to this dependency (i.e.
        /// [`Dependency::alias()`]).
        alias: String,
        /// The item's name.
        name: String,
    },
}

/// The root package that directs package resolution - typically used with
/// [`resolve()`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPackage {
    package_name: String,
    version: Version,
    dependencies: Vec<Dependency>,
}

impl RootPackage {
    pub fn new(package_name: String, version: Version, dependencies: Vec<Dependency>) -> Self {
        Self {
            package_name,
            version,
            dependencies,
        }
    }

    pub fn from_webc_metadata(_manifest: &Manifest) -> Self {
        todo!();
    }

    pub async fn from_registry(
        specifier: &PackageSpecifier,
        registry: &(impl Registry + ?Sized),
    ) -> Result<RootPackage, Error> {
        let summaries = registry.query(specifier).await?;

        match summaries
            .into_iter()
            .max_by(|left, right| left.version.cmp(&right.version))
        {
            Some(Summary {
                package_name,
                version,
                dependencies,
                ..
            }) => Ok(RootPackage {
                package_name,
                version,
                dependencies,
            }),
            None => Err(Error::msg(
                "Unable to find a package matching that specifier",
            )),
        }
    }
}

/// An identifier for a package within a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    package_name: String,
    version: Version,
    source: SourceId,
}

/// A dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    root: PackageId,
    dependencies: HashMap<PackageId, HashMap<String, PackageId>>,
    summaries: HashMap<PackageId, Summary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolve {
    graph: DependencyGraph,
    package: ResolvedPackage,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedCommand {
    pub metadata: webc::metadata::Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemMapping {
    pub mount_path: PathBuf,
    pub volume_name: String,
    pub package: PackageId,
}

/// A package that has been resolved, but is not yet runnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub root_package: PackageId,
    pub commands: BTreeMap<String, ResolvedCommand>,
    pub atoms: Vec<(String, ItemLocation)>,
    pub entrypoint: Option<String>,
    /// A mapping from paths to the volumes that should be mounted there.
    pub filesystem: Vec<FileSystemMapping>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn parse_some_package_specifiers() {
        let inputs = [
            (
                "first",
                PackageSpecifier::Registry {
                    full_name: "first".to_string(),
                    version: VersionReq::STAR,
                },
            ),
            (
                "namespace/package",
                PackageSpecifier::Registry {
                    full_name: "namespace/package".to_string(),
                    version: VersionReq::STAR,
                },
            ),
            (
                "namespace/package@1.0.0",
                PackageSpecifier::Registry {
                    full_name: "namespace/package".to_string(),
                    version: "1.0.0".parse().unwrap(),
                },
            ),
        ];

        for (src, expected) in inputs {
            let parsed = PackageSpecifier::from_str(src).unwrap();
            assert_eq!(parsed, expected);
        }
    }
}
