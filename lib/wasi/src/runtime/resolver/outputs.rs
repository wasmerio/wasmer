use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Display, Formatter},
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
    pub package: PackageId,
}

/// An identifier for a package within a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub package_name: String,
    pub version: Version,
    pub source: SourceId,
}

impl Display for PackageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let PackageId {
            package_name,
            version,
            source,
        } = self;
        write!(f, "{package_name} {version}")?;

        let url = source.url();

        match source.kind() {
            super::SourceKind::Path => match url.to_file_path() {
                Ok(path) => write!(f, " ({})", path.display()),
                Err(_) => write!(f, " ({url})"),
            },
            super::SourceKind::Url => write!(f, " ({url})"),
            super::SourceKind::Registry => write!(f, " (registry+{url})"),
            super::SourceKind::LocalRegistry => write!(f, " (local+{url})"),
        }
    }
}

/// A dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyGraph {
    pub root: PackageId,
    pub dependencies: HashMap<PackageId, HashMap<String, PackageId>>,
    pub summaries: HashMap<PackageId, Summary>,
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
