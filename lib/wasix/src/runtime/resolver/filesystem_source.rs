use anyhow::Context;
use wasmer_config::package::{PackageHash, PackageId, PackageSource};
use webc::compat::Container;

use crate::runtime::resolver::{
    DistributionInfo, PackageInfo, PackageSummary, QueryError, Source, WebcHash,
};

/// A [`Source`] that knows how to query files on the filesystem.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileSystemSource {}

#[async_trait::async_trait]
impl Source for FileSystemSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        let path = match package {
            PackageSource::Path(path) => {
                let path = std::path::PathBuf::from(path);
                path.canonicalize().with_context(|| {
                    format!(
                        "Unable to get the canonical form for \"{}\"",
                        path.display()
                    )
                })?
            }
            _ => return Err(QueryError::Unsupported),
        };

        let webc_sha256 = crate::block_in_place(|| WebcHash::for_file(&path))
            .with_context(|| format!("Unable to hash \"{}\"", path.display()))?;
        let container = crate::block_in_place(|| Container::from_disk(&path))
            .with_context(|| format!("Unable to parse \"{}\"", path.display()))?;

        let url = crate::runtime::resolver::utils::url_from_file_path(&path)
            .ok_or_else(|| anyhow::anyhow!("Unable to turn \"{}\" into a URL", path.display()))?;

        let id = PackageInfo::package_id_from_manifest(container.manifest())
            .context("Unable to determine the package's ID")?
            .unwrap_or_else(|| PackageId::from(PackageHash::from_sha256_bytes(webc_sha256.0)));

        let pkg = PackageInfo::from_manifest(id, container.manifest(), container.version())
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
