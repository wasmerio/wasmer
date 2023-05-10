use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    path::PathBuf,
    str::FromStr,
};

use anyhow::{Context, Error};
use semver::{Version, VersionReq};
use url::Url;
use webc::{compat::Container, metadata::Manifest};

use crate::bin_factory::BinaryPackage;

/// Given a [`RootPackage`], resolve its dependency graph and figure out
/// how it could be reconstituted.
pub async fn resolve(
    _root: &RootPackage,
    _registry: &impl Registry,
) -> Result<(ResolvedPackage, DependencyGraph), Error> {
    todo!();
}

/// Take the results of [`resolve()`] and use the loaded packages to turn
/// it into a runnable [`BinaryPackage`].
pub fn reconstitute(
    _pkg: &ResolvedPackage,
    _graph: &DependencyGraph,
    _packages: &HashMap<PackageId, Container>,
) -> Result<BinaryPackage, Error> {
    todo!();
}

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

/// Load a [`BinaryPackage`] from a [`PackageSpecifier`].
///
/// # Note for Implementations
///
/// Internally, you will probably want to use [`resolve()`] and
/// [`reconstitute()`] when loading packages.
///
/// Package loading and intermediate artefacts should also be cached where
/// possible.
#[async_trait::async_trait]
pub trait PackageResolver: Debug {
    async fn load_package(&self, pkg: &PackageSpecifier) -> Result<BinaryPackage, Error>;
    async fn load_webc(&self, webc: &Container) -> Result<BinaryPackage, Error>;
}

/// A component that tracks all available packages, allowing users to query
/// dependency information.
#[async_trait::async_trait]
pub trait Registry: Debug {
    async fn query(&self, pkg: &PackageSpecifier) -> Result<Vec<Summary>, Error>;
}

#[async_trait::async_trait]
impl<D, R> Registry for D
where
    D: std::ops::Deref<Target = R> + Debug + Send + Sync,
    R: Registry + Send + Sync + 'static,
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

/// Some metadata a [`Source`] can provide about a package without needing
/// to download the entire `*.webc` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The package's full name (i.e. `wasmer/wapm2pirita`).
    package_name: String,
    /// The package version.
    version: Version,
    /// A URL that can be used to download the `*.webc` file.
    webc: Url,
    /// A SHA-256 checksum for the `*.webc` file.
    webc_sha256: [u8; 32],
    /// Any dependencies this package may have.
    dependencies: Vec<Dependency>,
    /// Commands this package exposes to the outside world.
    commands: Vec<Command>,
    /// The [`Source`] this [`Summary`] came from.
    source: SourceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    name: String,
    atom: ItemLocation,
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
        /// [`Dependency::alias`]).
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
        registry: &impl Registry,
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
    dependencies: HashMap<PackageId, Vec<(String, PackageId)>>,
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
