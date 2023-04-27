mod default;

use crate::bin_factory::BinaryPackage;

pub use self::default::DefaultResolver;

use std::{collections::BTreeMap, path::PathBuf};

use webc::metadata::Manifest;

#[async_trait::async_trait]
pub trait PackageResolver: std::fmt::Debug {
    async fn load_manifest(&self, pkg: WebcIdentifier) -> Result<Manifest, ResolverError>;

    /// Resolve a package, loading all dependencies.
    async fn resolve_package(&self, pkg: WebcIdentifier) -> Result<ResolvedPackage, ResolverError>;
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct WebcIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub commands: BTreeMap<String, ResolvedCommand>,
    pub entrypoint: Option<String>,
    /// A mapping from paths to the volumes that should be mounted there.
    pub filesystem: Vec<FileSystemMapping>,
}

impl From<ResolvedPackage> for BinaryPackage {
    fn from(_: ResolvedPackage) -> Self {
        todo!()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedCommand {
    pub metadata: webc::metadata::Command,
}

#[derive(Debug, Clone)]
pub struct FileSystemMapping {
    pub mount_path: PathBuf,
    pub volume: webc::compat::Volume,
}
