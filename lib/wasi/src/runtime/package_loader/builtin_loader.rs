use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{ErrorKind, Write as _},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use anyhow::{Context, Error};
use bytes::Bytes;
use tempfile::NamedTempFile;
use webc::{
    compat::{Container, ContainerError},
    DetectError,
};

use crate::{
    http::{HttpClient, HttpRequest, HttpResponse, USER_AGENT},
    runtime::{
        package_loader::PackageLoader,
        resolver::{Summary, WebcHash},
    },
};

/// The builtin [`PackageLoader`] that is used by the `wasmer` CLI and
/// respects `$WASMER_HOME`.
#[derive(Debug)]
pub struct BuiltinLoader {
    client: Arc<dyn HttpClient + Send + Sync>,
    in_memory: InMemoryCache,
    fs: FileSystemCache,
}

impl BuiltinLoader {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        let client = crate::http::default_http_client().unwrap();
        BuiltinLoader::new_with_client(cache_dir, Arc::new(client))
    }

    pub fn new_with_client(
        cache_dir: impl Into<PathBuf>,
        client: Arc<dyn HttpClient + Send + Sync>,
    ) -> Self {
        BuiltinLoader {
            fs: FileSystemCache {
                cache_dir: cache_dir.into(),
            },
            in_memory: InMemoryCache::default(),
            client,
        }
    }

    /// Create a new [`BuiltinLoader`] based on `$WASMER_HOME` and the global
    /// Wasmer config.
    pub fn from_env() -> Result<Self, Error> {
        let wasmer_home = discover_wasmer_home().context("Unable to determine $WASMER_HOME")?;
        let client = crate::http::default_http_client().context("No HTTP client available")?;
        Ok(BuiltinLoader::new_with_client(
            wasmer_home.join("checkouts"),
            Arc::new(client),
        ))
    }

    #[tracing::instrument(skip_all, fields(pkg.hash=?hash))]
    async fn get_cached(&self, hash: &WebcHash) -> Result<Option<Container>, Error> {
        if let Some(cached) = self.in_memory.lookup(hash) {
            return Ok(Some(cached));
        }

        if let Some(cached) = self.fs.lookup(hash).await? {
            // Note: We want to propagate it to the in-memory cache, too
            self.in_memory.save(&cached, *hash);
            return Ok(Some(cached));
        }

        Ok(None)
    }

    async fn download(&self, summary: &Summary) -> Result<Bytes, Error> {
        if summary.webc.scheme() == "file" {
            if let Ok(path) = summary.webc.to_file_path() {
                // FIXME: This will block the thread
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
                return Ok(bytes.into());
            }
        }

        let request = HttpRequest {
            url: summary.webc.to_string(),
            method: "GET".to_string(),
            headers: vec![
                ("Accept".to_string(), "application/webc".to_string()),
                ("User-Agent".to_string(), USER_AGENT.to_string()),
            ],
            body: None,
            options: Default::default(),
        };

        let HttpResponse {
            body,
            ok,
            status,
            status_text,
            ..
        } = self.client.request(request).await?;

        if !ok {
            anyhow::bail!("{status} {status_text}");
        }

        let body = body.context("The response didn't contain a body")?;

        Ok(body.into())
    }

    async fn save_and_load_as_mmapped(
        &self,
        webc: &[u8],
        summary: &Summary,
    ) -> Result<Container, Error> {
        // First, save it to disk
        self.fs.save(webc, summary).await?;

        // Now try to load it again. The resulting container should use
        // a memory-mapped file rather than an in-memory buffer.
        match self.fs.lookup(&summary.webc_sha256).await? {
            Some(container) => {
                // we also want to make sure it's in the in-memory cache
                self.in_memory.save(&container, summary.webc_sha256);

                Ok(container)
            }
            None => {
                // Something really weird has occurred and we can't see the
                // saved file. Just error out and let the fallback code do its
                // thing.
                Err(Error::msg("Unable to load the downloaded memory from disk"))
            }
        }
    }
}

#[async_trait::async_trait]
impl PackageLoader for BuiltinLoader {
    async fn load(&self, summary: &Summary) -> Result<Container, Error> {
        if let Some(container) = self.get_cached(&summary.webc_sha256).await? {
            return Ok(container);
        }

        // looks like we had a cache miss and need to download it manually
        let bytes = self
            .download(summary)
            .await
            .with_context(|| format!("Unable to download \"{}\"", summary.webc))?;

        // We want to cache the container we downloaded, but we want to do it
        // in a smart way to keep memory usage down.

        match self.save_and_load_as_mmapped(&bytes, summary).await {
            Ok(container) => {
                // The happy path - we've saved to both caches and loaded the
                // container from disk (hopefully using mmap) so we're done.
                return Ok(container);
            }
            Err(e) => {
                tracing::warn!(
                    error=&*e,
                    pkg.name=%summary.package_name,
                    pkg.version=%summary.version,
                    pkg.hash=?summary.webc_sha256,
                    pkg.url=%summary.webc,
                    "Unable to save the downloaded package to disk",
                );
                // The sad path - looks like we'll need to keep the whole thing
                // in memory.
                let container = Container::from_bytes(bytes)?;
                // We still want to cache it, of course
                self.in_memory.save(&container, summary.webc_sha256);
                Ok(container)
            }
        }
    }
}

fn discover_wasmer_home() -> Option<PathBuf> {
    // TODO: We should reuse the same logic from the wasmer CLI.
    std::env::var("WASMER_HOME")
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

        match Container::from_disk(&path) {
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

    async fn save(&self, webc: &[u8], summary: &Summary) -> Result<(), Error> {
        let path = self.path(&summary.webc_sha256);

        let parent = path.parent().expect("Always within cache_dir");

        std::fs::create_dir_all(parent)
            .with_context(|| format!("Unable to create \"{}\"", parent.display()))?;

        let mut temp = NamedTempFile::new_in(parent)?;
        temp.write_all(webc)?;
        temp.flush()?;
        temp.as_file_mut().sync_all()?;
        temp.persist(path)?;

        Ok(())
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
    use tempfile::TempDir;

    use crate::{
        http::{HttpRequest, HttpResponse},
        runtime::resolver::{SourceId, SourceKind},
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../../c-api/examples/assets/python-0.1.0.wasmer");

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

    #[tokio::test]
    async fn cache_misses_will_trigger_a_download() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([HttpResponse {
            pos: 0,
            body: Some(PYTHON.to_vec()),
            ok: true,
            redirected: false,
            status: 200,
            status_text: "OK".to_string(),
            headers: Vec::new(),
        }]));
        let loader = BuiltinLoader::new_with_client(temp.path(), client.clone());
        let summary = Summary {
            package_name: "python/python".to_string(),
            version: "0.1.0".parse().unwrap(),
            webc: "https://wapm.io/python/python".parse().unwrap(),
            webc_sha256: [0xaa; 32].into(),
            dependencies: Vec::new(),
            commands: Vec::new(),
            source: SourceId::new(
                SourceKind::Url,
                "https://registry.wapm.io/graphql".parse().unwrap(),
            ),
            entrypoint: Some("asdf".to_string()),
        };

        let container = loader.load(&summary).await.unwrap();

        // A HTTP request was sent
        let requests = client.requests.lock().unwrap();
        let request = &requests[0];
        assert_eq!(request.url, summary.webc.to_string());
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.headers,
            [
                ("Accept".to_string(), "application/webc".to_string()),
                ("User-Agent".to_string(), USER_AGENT.to_string()),
            ]
        );
        // Make sure we got the right package
        let manifest = container.manifest();
        assert_eq!(manifest.entrypoint.as_deref(), Some("python"));
        // it should have been automatically saved to disk
        let path = loader.fs.path(&summary.webc_sha256);
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), PYTHON);
        // and cached in memory for next time
        let in_memory = loader.in_memory.0.read().unwrap();
        assert!(in_memory.contains_key(&summary.webc_sha256));
    }
}
