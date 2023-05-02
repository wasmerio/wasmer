use std::path::PathBuf;

use anyhow::Context;
use url::Url;

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{types::ResolverError, types::WebcIdentifier, PackageResolver},
};

/// A [`PackageResolver`] that will resolve packages by fetching them from the
/// WAPM registry.
///
/// Any downloaded assets will be cached on disk.
#[derive(Debug, Clone)]
pub struct RegistryResolver {
    cache_dir: PathBuf,
    registry_endpoint: Url,
    /// A list of [`BinaryPackage`]s that have already been loaded into memory
    /// by the user.
    // TODO: Remove this "preload" hack and update the snapshot tests to
    // use a local registry instead of "--include-webc"
    preloaded: Vec<BinaryPackage>,
}

impl RegistryResolver {
    pub const WAPM_DEV_ENDPOINT: &str = "https://registry.wapm.dev/graphql";
    pub const WAPM_PROD_ENDPOINT: &str = "https://registry.wapm.io/graphql";

    pub fn new(cache_dir: impl Into<PathBuf>, registry_endpoint: Url) -> Self {
        RegistryResolver {
            cache_dir: cache_dir.into(),
            registry_endpoint,
            preloaded: Vec::new(),
        }
    }

    /// Create a [`RegistryResolver`] using the current Wasmer toolchain
    /// installation.
    pub fn from_env() -> Result<Self, anyhow::Error> {
        // FIXME: respect active registry setting in wasmer.toml
        let wasmer_home = std::env::var_os("WASMER_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                #[allow(deprecated)]
                std::env::home_dir().map(|home| home.join(".wasmer"))
            })
            .context("Unable to determine Wasmer's home directory")?;

        let endpoint = RegistryResolver::WAPM_PROD_ENDPOINT.parse()?;

        Ok(RegistryResolver::new(wasmer_home, endpoint))
    }

    /// Add a preloaded [`BinaryPackage`] to the list of preloaded packages.
    ///
    /// The [`RegistryResolver`] adds a mechanism that allows you to "preload" a
    /// [`BinaryPackage`] that already exists in memory. The
    /// [`PackageResolver::resolve_package()`] method will first check this list
    /// for a compatible package before checking WAPM.
    ///
    /// **This mechanism should only be used for testing**. Expect it to be
    /// removed in future versions in favour of a local registry.
    pub fn add_preload(&mut self, pkg: BinaryPackage) -> &mut Self {
        self.preloaded.push(pkg);
        self
    }

    fn lookup_preloaded(&self, pkg: &WebcIdentifier) -> Option<&BinaryPackage> {
        self.preloaded.iter().find(|candidate| {
            candidate.package_name == pkg.full_name && pkg.version.matches(&candidate.version)
        })
    }
}

#[async_trait::async_trait]
impl PackageResolver for RegistryResolver {
    async fn resolve_package(
        &self,
        pkg: &WebcIdentifier,
        client: &(dyn HttpClient + Send + Sync),
    ) -> Result<BinaryPackage, ResolverError> {
        if let Some(preloaded) = self.lookup_preloaded(pkg) {
            return Ok(preloaded.clone());
        }

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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    #[cfg_attr(not(feature = "host-reqwest"), ignore = "Requires a HTTP client")]
    async fn resolved_webc_files_are_cached_locally() {
        let temp = TempDir::new().unwrap();
        let resolver = RegistryResolver::new(
            temp.path(),
            RegistryResolver::WAPM_PROD_ENDPOINT.parse().unwrap(),
        );
        let client = crate::http::default_http_client().expect("This test requires a HTTP client");
        let ident = WebcIdentifier::parse("wasmer/sha2@0.1.0").unwrap();

        let pkg = resolver.resolve_package(&ident, &client).await.unwrap();

        assert_eq!(pkg.package_name, "wasmer/sha2");
        assert_eq!(pkg.version.to_string(), "0.1.0");
        let filenames: Vec<_> = temp
            .path()
            .read_dir()
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            filenames,
            ["wasmer_sha2_sha2-0.1.0-2ada887a-9bb8-11ed-82ff-b2315a79a72a.webc"]
        );
    }
}
