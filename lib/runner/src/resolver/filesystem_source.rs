use anyhow::Context;
use webc::compat::Container;

use crate::resolver::{
    DistributionInfo, PackageInfo, PackageSpecifier, PackageSummary, QueryError, Source, WebcHash,
};

/// A [`Source`] that knows how to query files on the filesystem.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileSystemSource {}

#[async_trait::async_trait]
impl Source for FileSystemSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError> {
        let path = match package {
            PackageSpecifier::Path(path) => path.canonicalize().with_context(|| {
                format!(
                    "Unable to get the canonical form for \"{}\"",
                    path.display()
                )
            })?,
            _ => return Err(QueryError::Unsupported),
        };

        #[cfg(target_arch = "wasm32")]
        let webc_sha256 = WebcHash::for_file(&path)
            .with_context(|| format!("Unable to hash \"{}\"", path.display()))?;
        #[cfg(not(target_arch = "wasm32"))]
        let webc_sha256 = tokio::task::block_in_place(|| WebcHash::for_file(&path))
            .with_context(|| format!("Unable to hash \"{}\"", path.display()))?;
        #[cfg(target_arch = "wasm32")]
        let container = Container::from_disk(&path)
            .with_context(|| format!("Unable to parse \"{}\"", path.display()))?;
        #[cfg(not(target_arch = "wasm32"))]
        let container = tokio::task::block_in_place(|| Container::from_disk(&path))
            .with_context(|| format!("Unable to parse \"{}\"", path.display()))?;

        let url = crate::resolver::utils::url_from_file_path(&path)
            .ok_or_else(|| anyhow::anyhow!("Unable to turn \"{}\" into a URL", path.display()))?;

        let pkg = PackageInfo::from_manifest(container.manifest())
            .context("Unable to determine the package's metadata")?;
        let summary = PackageSummary {
            pkg,
            dist: DistributionInfo {
                webc: url,
                webc_sha256,
            },
        };

        Ok(vec![summary])
    }
}
