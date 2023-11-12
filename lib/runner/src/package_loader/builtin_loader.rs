use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{ErrorKind, Write as _},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use anyhow::{Context, Error};
use bytes::Bytes;
use http::{HeaderMap, Method};
use tempfile::NamedTempFile;
use webc::{
    compat::{Container, ContainerError},
    DetectError,
};

use crate::{
    binary_package::BinaryPackage,
    http::{HttpClient, HttpRequest, USER_AGENT},
    package_loader::PackageLoader,
    resolver::{DistributionInfo, PackageSummary, Resolution, WebcHash},
};

/// The builtin [`PackageLoader`] that is used by the `wasmer` CLI and
/// respects `$WASMER_DIR`.
#[derive(Debug)]
pub struct BuiltinPackageLoader {
    client: Arc<dyn HttpClient + Send + Sync>,
    in_memory: InMemoryCache,
    cache: Option<FileSystemCache>,
}

impl BuiltinPackageLoader {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        let client = crate::http::default_http_client().unwrap();
        BuiltinPackageLoader::new_with_client(cache_dir, Arc::new(client))
    }

    pub fn new_with_client(
        cache_dir: impl Into<PathBuf>,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Self {
        BuiltinPackageLoader {
            cache: Some(FileSystemCache {
                cache_dir: cache_dir.into(),
            }),
            in_memory: InMemoryCache::default(),
            client,
        }
    }

    pub fn new_only_client(client: Arc<dyn HttpClient + Send + Sync>) -> Self {
        BuiltinPackageLoader {
            cache: None,
            in_memory: InMemoryCache::default(),
            client,
        }
    }

    /// Create a new [`BuiltinPackageLoader`] based on `$WASMER_DIR` and the
    /// global Wasmer config.
    pub fn from_env() -> Result<Self, Error> {
        let wasmer_dir = discover_wasmer_dir().context("Unable to determine $WASMER_DIR")?;
        let client = crate::http::default_http_client().context("No HTTP client available")?;
        let cache_dir = wasmer_dir.join("cache").join("");

        Ok(BuiltinPackageLoader::new_with_client(
            cache_dir,
            Arc::new(client),
        ))
    }

    /// Insert a container into the in-memory hash.
    pub fn insert_cached(&self, hash: WebcHash, container: &Container) {
        self.in_memory.save(container, hash);
    }

    #[tracing::instrument(level = "debug", skip_all, fields(pkg.hash=%hash))]
    async fn get_cached(&self, hash: &WebcHash) -> Result<Option<Container>, Error> {
        if let Some(cached) = self.in_memory.lookup(hash) {
            return Ok(Some(cached));
        }

        if let Some(cache) = self.cache.as_ref() {
            if let Some(cached) = cache.lookup(hash).await? {
                // Note: We want to propagate it to the in-memory cache, too
                tracing::debug!("Copying from the filesystem cache to the in-memory cache");
                self.in_memory.save(&cached, *hash);
                return Ok(Some(cached));
            }
        }

        Ok(None)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(%dist.webc, %dist.webc_sha256))]
    async fn download(&self, dist: &DistributionInfo) -> Result<Bytes, Error> {
        if dist.webc.scheme() == "file" {
            match crate::resolver::utils::file_path_from_url(&dist.webc) {
                Ok(path) => {
                    // FIXME: This will block the thread
                    let bytes = std::fs::read(&path)
                        .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
                    return Ok(bytes.into());
                }
                Err(e) => {
                    tracing::debug!(
                        url=%dist.webc,
                        error=&*e,
                        "Unable to convert the file:// URL to a path",
                    );
                }
            }
        }

        let request = HttpRequest {
            url: dist.webc.clone(),
            method: Method::GET,
            headers: headers(),
            body: None,
            options: Default::default(),
        };

        tracing::debug!(%request.url, %request.method, "Downloading a webc file");
        tracing::trace!(?request.headers);

        let response = self.client.request(request).await?;

        tracing::trace!(
            %response.status,
            %response.redirected,
            ?response.headers,
            response.len=response.body.as_ref().map(|body| body.len()),
            "Received a response",
        );

        if !response.is_ok() {
            let url = &dist.webc;
            return Err(crate::resolver::utils::http_error(&response)
                .context(format!("The GET request to \"{url}\" failed")));
        }

        let body = response
            .body
            .context("The response didn't contain a body")?;

        Ok(body.into())
    }
}

#[async_trait::async_trait]
impl PackageLoader for BuiltinPackageLoader {
    #[tracing::instrument(
        level="debug",
        skip_all,
        fields(
            pkg.name=summary.pkg.name.as_str(),
            pkg.version=%summary.pkg.version,
        ),
    )]
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error> {
        if let Some(container) = self.get_cached(&summary.dist.webc_sha256).await? {
            tracing::debug!("Cache hit!");
            return Ok(container);
        }

        // looks like we had a cache miss and need to download it manually
        let bytes = self
            .download(&summary.dist)
            .await
            .with_context(|| format!("Unable to download \"{}\"", summary.dist.webc))?;

        // We want to cache the container we downloaded, but we want to do it
        // in a smart way to keep memory usage down.

        if let Some(cache) = &self.cache {
            match cache.save_and_load_as_mmapped(&bytes, &summary.dist).await {
                Ok(container) => {
                    tracing::debug!("Cached to disk");
                    self.in_memory.save(&container, summary.dist.webc_sha256);
                    // The happy path - we've saved to both caches and loaded the
                    // container from disk (hopefully using mmap) so we're done.
                    return Ok(container);
                }
                Err(e) => {
                    tracing::warn!(
                        error=&*e,
                        pkg.name=%summary.pkg.name,
                        pkg.version=%summary.pkg.version,
                        pkg.hash=%summary.dist.webc_sha256,
                        pkg.url=%summary.dist.webc,
                        "Unable to save the downloaded package to disk",
                    );
                }
            }
        }

        // The sad path - looks like we don't have a filesystem cache so we'll
        // need to keep the whole thing in memory.
        let container = Container::from_bytes(bytes)?;
        // We still want to cache it in memory, of course
        self.in_memory.save(&container, summary.dist.webc_sha256);
        Ok(container)
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
    ) -> Result<BinaryPackage, Error> {
        super::load_package_tree(root, self, resolution).await
    }
}

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Accept", "application/webc".parse().unwrap());
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers
}

fn discover_wasmer_dir() -> Option<PathBuf> {
    // TODO: We should reuse the same logic from the wasmer CLI.
    std::env::var("WASMER_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            #[allow(deprecated)]
            std::env::home_dir().map(|home| home.join(".wasmer"))
        })
}

// FIXME: This implementation will block the async runtime and should use
// some sort of spawn_blocking() call to run it in the background.
#[derive(Debug)]
struct FileSystemCache {
    cache_dir: PathBuf,
}

impl FileSystemCache {
    async fn lookup(&self, hash: &WebcHash) -> Result<Option<Container>, Error> {
        let path = self.path(hash);

        #[cfg(target_arch = "wasm32")]
        let container = Container::from_disk(&path);
        #[cfg(not(target_arch = "wasm32"))]
        let container = tokio::task::block_in_place(|| Container::from_disk(&path));
        match container {
            Ok(c) => Ok(Some(c)),
            Err(ContainerError::Open { error, .. })
            | Err(ContainerError::Read { error, .. })
            | Err(ContainerError::Detect(DetectError::Io(error)))
                if error.kind() == ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(e) => {
                let msg = format!("Unable to read \"{}\"", path.display());
                Err(Error::new(e).context(msg))
            }
        }
    }

    async fn save(&self, webc: &[u8], dist: &DistributionInfo) -> Result<(), Error> {
        let path = self.path(&dist.webc_sha256);

        let parent = path.parent().expect("Always within cache_dir");

        std::fs::create_dir_all(parent)
            .with_context(|| format!("Unable to create \"{}\"", parent.display()))?;

        let mut temp = NamedTempFile::new_in(parent)?;
        temp.write_all(webc)?;
        temp.flush()?;
        temp.as_file_mut().sync_all()?;
        temp.persist(&path)?;

        tracing::debug!(
            pkg.hash=%dist.webc_sha256,
            pkg.url=%dist.webc,
            path=%path.display(),
            num_bytes=webc.len(),
            "Saved to disk",
        );

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn save_and_load_as_mmapped(
        &self,
        webc: &[u8],
        dist: &DistributionInfo,
    ) -> Result<Container, Error> {
        // First, save it to disk
        self.save(webc, dist).await?;

        // Now try to load it again. The resulting container should use
        // a memory-mapped file rather than an in-memory buffer.
        match self.lookup(&dist.webc_sha256).await? {
            Some(container) => Ok(container),
            None => {
                // Something really weird has occurred and we can't see the
                // saved file. Just error out and let the fallback code do its
                // thing.
                Err(Error::msg("Unable to load the downloaded memory from disk"))
            }
        }
    }

    fn path(&self, hash: &WebcHash) -> PathBuf {
        let hash = hash.as_bytes();
        let mut filename = String::with_capacity(hash.len() * 2);
        for b in hash {
            write!(filename, "{b:02x}").unwrap();
        }
        filename.push_str(".bin");

        self.cache_dir.join(filename)
    }
}

#[derive(Debug, Default)]
struct InMemoryCache(RwLock<HashMap<WebcHash, Container>>);

impl InMemoryCache {
    fn lookup(&self, hash: &WebcHash) -> Option<Container> {
        self.0.read().unwrap().get(hash).cloned()
    }

    fn save(&self, container: &Container, hash: WebcHash) {
        let mut cache = self.0.write().unwrap();
        cache.entry(hash).or_insert_with(|| container.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use futures::future::BoxFuture;
    use http::{HeaderMap, StatusCode};
    use tempfile::TempDir;

    use crate::{
        http::{HttpRequest, HttpResponse},
        resolver::PackageInfo,
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");

    #[derive(Debug)]
    pub(crate) struct DummyClient {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<VecDeque<HttpResponse>>,
    }

    impl DummyClient {
        pub fn with_responses(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
            DummyClient {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    impl HttpClient for DummyClient {
        fn request(
            &self,
            request: HttpRequest,
        ) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
            let response = self.responses.lock().unwrap().pop_front().unwrap();
            self.requests.lock().unwrap().push(request);
            Box::pin(async { Ok(response) })
        }
    }

    async fn cache_misses_will_trigger_a_download_internal() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([HttpResponse {
            body: Some(PYTHON.to_vec()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        }]));
        let loader = BuiltinPackageLoader::new_with_client(temp.path(), client.clone());
        let summary = PackageSummary {
            pkg: PackageInfo {
                name: "python/python".to_string(),
                version: "0.1.0".parse().unwrap(),
                dependencies: Vec::new(),
                commands: Vec::new(),
                entrypoint: Some("asdf".to_string()),
                filesystem: Vec::new(),
            },
            dist: DistributionInfo {
                webc: "https://wasmer.io/python/python".parse().unwrap(),
                webc_sha256: [0xaa; 32].into(),
            },
        };

        let container = loader.load(&summary).await.unwrap();

        // A HTTP request was sent
        let requests = client.requests.lock().unwrap();
        let request = &requests[0];
        assert_eq!(request.url, summary.dist.webc);
        assert_eq!(request.method, "GET");
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.headers["Accept"], "application/webc");
        assert_eq!(request.headers["User-Agent"], USER_AGENT);
        // Make sure we got the right package
        let manifest = container.manifest();
        assert_eq!(manifest.entrypoint.as_deref(), Some("python"));
        // it should have been automatically saved to disk
        let path = loader
            .cache
            .as_ref()
            .unwrap()
            .path(&summary.dist.webc_sha256);
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), PYTHON);
        // and cached in memory for next time
        let in_memory = loader.in_memory.0.read().unwrap();
        assert!(in_memory.contains_key(&summary.dist.webc_sha256));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn cache_misses_will_trigger_a_download() {
        cache_misses_will_trigger_a_download_internal().await
    }

    #[cfg(target_arch = "wasm32")]
    #[tokio::test()]
    async fn cache_misses_will_trigger_a_download() {
        cache_misses_will_trigger_a_download_internal().await
    }
}
