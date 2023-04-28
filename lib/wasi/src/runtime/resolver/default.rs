use std::path::PathBuf;

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{types::ResolverError, types::WebcIdentifier, PackageResolver},
};

/// The default package resolver, backed by WAPM.
///
/// Any downloaded assets will be cached on disk.
#[derive(Debug, Clone)]
pub struct DefaultResolver {
    cache_dir: PathBuf,
}

impl DefaultResolver {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        DefaultResolver {
            cache_dir: cache_dir.into(),
        }
    }
}

impl Default for DefaultResolver {
    fn default() -> Self {
        // TODO: Reuse the same logic as wasmer-cli
        let wasmer_home = std::env::var_os("WASMER_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                #[allow(deprecated)]
                std::env::home_dir().map(|home| home.join(".wasmer"))
            })
            .unwrap();

        DefaultResolver::new(wasmer_home)
    }
}

#[async_trait::async_trait]
impl PackageResolver for DefaultResolver {
    async fn resolve_package(
        &self,
        pkg: WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        crate::wapm::fetch_webc(&self.cache_dir, &pkg.full_name, client)
            .await
            .map_err(|e| ResolverError::Other(e.into()))
    }
}
