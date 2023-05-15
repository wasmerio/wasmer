use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use semver::Version;

use crate::runtime::resolver::{SourceId, Summary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub package: ResolvedPackage,
    pub graph: DependencyGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLocation {
    /// The item's original name.
    pub name: String,
    /// The package this item comes from.
    pub pkg: PackageId,
}

/// An identifier for a package within a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub package_name: String,
    pub version: Version,
    pub source: SourceId,
}

/// A dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    pub root: PackageId,
    pub dependencies: HashMap<PackageId, HashMap<String, PackageId>>,
    pub summaries: HashMap<PackageId, Summary>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedCommand {
    pub name: String,
    pub package: PackageId,
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
    pub commands: BTreeMap<String, ItemLocation>,
    pub atoms: Vec<(String, ItemLocation)>,
    pub entrypoint: Option<String>,
    /// A mapping from paths to the volumes that should be mounted there.
    pub filesystem: Vec<FileSystemMapping>,
}
