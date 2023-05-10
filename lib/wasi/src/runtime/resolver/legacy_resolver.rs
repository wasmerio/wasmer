use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Error};
use semver::VersionReq;
use url::Url;
use webc::Container;

use crate::{
    bin_factory::BinaryPackage,
    http::HttpClient,
    runtime::resolver::{PackageResolver, PackageSpecifier},
};

/// A [`PackageResolver`] that will resolve packages by fetching them from the
/// WAPM registry.
///
/// Any downloaded assets will be cached on disk.
///
/// # Footguns
///
/// This implementation includes a number of potential footguns and **should not
/// be used in production**.
///
/// All loading of WEBCs from disk is done using blocking IO, which will block
/// the async runtime thread.
///
/// It also doesn't do any dependency resolution. That means loading a package
/// which has dependencies will probably produce an unusable [`BinaryPackage`].
///
/// Prefer to use [`crate::runtime::resolver::BuiltinResolver`] instead.
#[derive(Debug, Clone)]
pub struct LegacyResolver {
    cache_dir: PathBuf,
    registry_endpoint: Url,
    /// A list of [`BinaryPackage`]s that have already been loaded into memory
    /// by the user.
    // TODO: Remove this "preload" hack and update the snapshot tests to
    // use a local registry instead of "--include-webc"
    preloaded: Vec<BinaryPackage>,
    client: Arc<dyn HttpClient + Send + Sync>,
}

impl LegacyResolver {
    pub const WAPM_DEV_ENDPOINT: &str = "https://registry.wapm.dev/graphql";
    pub const WAPM_PROD_ENDPOINT: &str = "https://registry.wapm.io/graphql";

    pub fn new(
        cache_dir: impl Into<PathBuf>,
        registry_endpoint: Url,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Self {
        LegacyResolver {
            cache_dir: cache_dir.into(),
            registry_endpoint,
            preloaded: Vec::new(),
            client,
        }
    }

    /// Create a [`RegistryResolver`] using the current Wasmer toolchain
    /// installation.
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let client = crate::http::default_http_client().context("No HTTP client available")?;

        LegacyResolver::from_env_with_client(client)
    }

    fn from_env_with_client(
        client: impl HttpClient + Send + Sync + 'static,
    ) -> Result<LegacyResolver, anyhow::Error> {
        // FIXME: respect active registry setting in wasmer.toml... We currently
        // do things the hard way because pulling in the wasmer-registry crate
        // would add loads of extra dependencies and make it harder to build
        // wasmer-wasix when "js" is enabled.
        let wasmer_home = std::env::var_os("WASMER_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                #[allow(deprecated)]
                std::env::home_dir().map(|home| home.join(".wasmer"))
            })
            .context("Unable to determine Wasmer's home directory")?;

        let endpoint = LegacyResolver::WAPM_PROD_ENDPOINT.parse()?;

        Ok(LegacyResolver::new(wasmer_home, endpoint, Arc::new(client)))
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

    fn lookup_preloaded(&self, full_name: &str, version: &VersionReq) -> Option<&BinaryPackage> {
        self.preloaded.iter().find(|candidate| {
            candidate.package_name == full_name && version.matches(&candidate.version)
        })
    }

    async fn load_from_registry(
        &self,
        full_name: &str,
        version: &VersionReq,
    ) -> Result<BinaryPackage, Error> {
        if let Some(preloaded) = self.lookup_preloaded(full_name, version) {
            return Ok(preloaded.clone());
        }

        crate::wapm::fetch_webc(
            &self.cache_dir,
            full_name,
            &self.client,
            &self.registry_endpoint,
        )
        .await
    }

    async fn load_url(&self, url: &Url) -> Result<BinaryPackage, Error> {
        let request = crate::http::HttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: vec![("Accept".to_string(), "application/webc".to_string())],
            body: None,
            options: crate::http::HttpRequestOptions::default(),
        };
        let response = self.client.request(request).await?;
        anyhow::ensure!(response.status == 200);
        let body = response
            .body
            .context("The response didn't contain a body")?;
        let container = Container::from_bytes(body)?;
        self.load_webc(&container).await
    }
}

#[async_trait::async_trait]
impl PackageResolver for LegacyResolver {
    async fn load_package(&self, pkg: &PackageSpecifier) -> Result<BinaryPackage, Error> {
        match pkg {
            PackageSpecifier::Registry { full_name, version } => {
                self.load_from_registry(full_name, version).await
            }
            PackageSpecifier::Url(url) => self.load_url(url).await,
            PackageSpecifier::Path(path) => {
                let container = Container::from_disk(path)?;
                self.load_webc(&container).await
            }
        }
    }

    async fn load_webc(&self, webc: &Container) -> Result<BinaryPackage, Error> {
        crate::wapm::parse_webc(webc)
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
        let client = crate::http::default_http_client().expect("This test requires a HTTP client");
        let resolver = LegacyResolver::new(
            temp.path(),
            LegacyResolver::WAPM_PROD_ENDPOINT.parse().unwrap(),
            Arc::new(client),
        );
        let ident: PackageSpecifier = "wasmer/sha2@0.1.0".parse().unwrap();

        let pkg = resolver.load_package(&ident).await.unwrap();

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
