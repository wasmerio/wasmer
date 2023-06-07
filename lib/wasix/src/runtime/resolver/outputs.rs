use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Display, Formatter},
    path::PathBuf,
    unreachable,
};

use semver::Version;

use crate::runtime::resolver::{DistributionInfo, PackageInfo};

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
    pub package: PackageId,
}

/// An identifier for a package within a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub package_name: String,
    pub version: Version,
}

impl Display for PackageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let PackageId {
            package_name,
            version,
        } = self;
        write!(f, "{package_name}@{version}")
    }
}

/// A dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    pub root: PackageId,
    pub dependencies: HashMap<PackageId, HashMap<String, PackageId>>,
    pub package_info: HashMap<PackageId, PackageInfo>,
    pub distribution: HashMap<PackageId, DistributionInfo>,
}

impl DependencyGraph {
    pub fn root_info(&self) -> &PackageInfo {
        match self.package_info.get(&self.root) {
            Some(info) => info,
            None => unreachable!(
                "The dependency graph should always have package info for the root package, {}",
                self.root
            ),
        }
    }
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
    pub entrypoint: Option<String>,
    /// A mapping from paths to the volumes that should be mounted there.
    pub filesystem: Vec<FileSystemMapping>,
}
