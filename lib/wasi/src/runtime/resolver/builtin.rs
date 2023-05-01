use std::path::PathBuf;

use anyhow::Context;
use url::Url;

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{types::ResolverError, types::WebcIdentifier, PackageResolver},
};

/// The builtin package resolver, backed by WAPM.
///
/// Any downloaded assets will be cached on disk.
#[derive(Debug, Clone)]
pub struct BuiltinResolver {
    cache_dir: PathBuf,
    registry_endpoint: Url,
}

impl BuiltinResolver {
    pub const WAPM_DEV_ENDPOINT: &str = "https://registry.wapm.dev/graphql";
    pub const WAPM_PROD_ENDPOINT: &str = "https://registry.wapm.io/graphql";

    pub fn new(cache_dir: impl Into<PathBuf>, registry_endpoint: Url) -> Self {
        BuiltinResolver {
            cache_dir: cache_dir.into(),
            registry_endpoint,
        }
    }

    /// Create a [`BuiltinResolver`] using the current Wasmer toolchain
    /// installation.
    pub fn from_env() -> Result<Self, anyhow::Error> {
        // TODO: Reuse the same logic as wasmer-cli
        let wasmer_home = std::env::var_os("WASMER_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                #[allow(deprecated)]
                std::env::home_dir().map(|home| home.join(".wasmer"))
            })
            .context("Unable to determine Wasmer's home directory")?;

        let endpoint = BuiltinResolver::WAPM_PROD_ENDPOINT.parse()?;

        Ok(BuiltinResolver::new(wasmer_home, endpoint))
    }
}

#[async_trait::async_trait]
impl PackageResolver for BuiltinResolver {
    async fn resolve_package(
        &self,
        pkg: &WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        crate::wapm::fetch_webc(
            &self.cache_dir,
            &pkg.full_name,
            client,
            &self.registry_endpoint,
        )
        .await
        .map_err(|e| ResolverError::Other(e.into()))
    }
}
