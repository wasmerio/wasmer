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
use url::Url;
use wasmer_package::{
    package::WasmerPackageError,
    utils::{from_bytes, from_disk},
};
use webc::DetectError;
use webc::{Container, ContainerError};

use crate::{
    bin_factory::BinaryPackage,
    http::{HttpClient, HttpRequest, USER_AGENT},
    runtime::{
        package_loader::PackageLoader,
        resolver::{DistributionInfo, PackageSummary, Resolution, WebcHash},
    },
};

/// The builtin [`PackageLoader`] that is used by the `wasmer` CLI and
/// respects `$WASMER_DIR`.
#[derive(Debug)]
pub struct BuiltinPackageLoader {
    client: Arc<dyn HttpClient + Send + Sync>,
    in_memory: InMemoryCache,
    cache: Option<FileSystemCache>,
    /// A mapping from hostnames to tokens
    tokens: HashMap<String, String>,

    hash_validation: HashIntegrityValidationMode,
}

/// Defines how to validate package hash integrity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashIntegrityValidationMode {
    /// Do not validate anything.
    /// Best for performance.
    NoValidate,
    /// Compute the image hash and produce a trace warning on hash mismatches.
    WarnOnHashMismatch,
    /// Compute the image hash and fail on a mismatch.
    FailOnHashMismatch,
}

impl BuiltinPackageLoader {
    pub fn new() -> Self {
        BuiltinPackageLoader {
            in_memory: InMemoryCache::default(),
            client: Arc::new(crate::http::default_http_client().unwrap()),
            cache: None,
            hash_validation: HashIntegrityValidationMode::NoValidate,
            tokens: HashMap::new(),
        }
    }

    /// Set the validation mode to apply after downloading an image.
    ///
    /// See [`HashIntegrityValidationMode`] for details.
    pub fn with_hash_validation_mode(mut self, mode: HashIntegrityValidationMode) -> Self {
        self.hash_validation = mode;
        self
    }

    pub fn with_cache_dir(self, cache_dir: impl Into<PathBuf>) -> Self {
        BuiltinPackageLoader {
            cache: Some(FileSystemCache {
                cache_dir: cache_dir.into(),
            }),
            ..self
        }
    }

    pub fn cache(&self) -> Option<&FileSystemCache> {
        self.cache.as_ref()
    }

    pub fn validate_cache(
        &self,
        mode: CacheValidationMode,
    ) -> Result<Vec<ImageHashMismatchError>, anyhow::Error> {
        let cache = self
            .cache
            .as_ref()
            .context("can not validate cache - no cache configured")?;

        let items = cache.validate_hashes()?;
        let mut errors = Vec::new();
        for (path, error) in items {
            match mode {
                CacheValidationMode::WarnOnMismatch => {
                    tracing::warn!(?error, "hash mismatch in cached image file");
                }
                CacheValidationMode::PruneOnMismatch => {
                    tracing::warn!(?error, "deleting cached image file due to hash mismatch");
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(fs_err) => {
                            tracing::error!(
                                path=%error.source,
                                ?fs_err,
                                "could not delete cached image file with hash mismatch"
                            );
                        }
                    }
                }
            }

            errors.push(error);
        }

        Ok(errors)
    }

    pub fn with_http_client(self, client: impl HttpClient + Send + Sync + 'static) -> Self {
        self.with_shared_http_client(Arc::new(client))
    }

    pub fn with_shared_http_client(self, client: Arc<dyn HttpClient + Send + Sync>) -> Self {
        BuiltinPackageLoader { client, ..self }
    }

    pub fn with_tokens<I, K, V>(mut self, tokens: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (hostname, token) in tokens {
            self = self.with_token(hostname, token);
        }

        self
    }

    /// Add an API token that will be used whenever sending requests to a
    /// particular hostname.
    ///
    /// Note that this uses [`Url::authority()`] when looking up tokens, so it
    /// will match both plain hostnames (e.g. `registry.wasmer.io`) and hosts
    /// with a port number (e.g. `localhost:8000`).
    pub fn with_token(mut self, hostname: impl Into<String>, token: impl Into<String>) -> Self {
        self.tokens.insert(hostname.into(), token.into());
        self
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

    /// Validate image contents with the specified validation mode.
    async fn validate_hash(
        image: &bytes::Bytes,
        mode: HashIntegrityValidationMode,
        info: &DistributionInfo,
    ) -> Result<(), anyhow::Error> {
        let info = info.clone();
        let image = image.clone();
        crate::spawn_blocking(move || Self::validate_hash_sync(&image, mode, &info))
            .await
            .context("tokio runtime failed")?
    }

    /// Validate image contents with the specified validation mode.
    fn validate_hash_sync(
        image: &[u8],
        mode: HashIntegrityValidationMode,
        info: &DistributionInfo,
    ) -> Result<(), anyhow::Error> {
        match mode {
            HashIntegrityValidationMode::NoValidate => {
                // Nothing to do.
                Ok(())
            }
            HashIntegrityValidationMode::WarnOnHashMismatch => {
                let actual_hash = WebcHash::sha256(image);
                if actual_hash != info.webc_sha256 {
                    tracing::warn!(%info.webc_sha256, %actual_hash, "image hash mismatch - actual image hash does not match the expected hash!");
                }
                Ok(())
            }
            HashIntegrityValidationMode::FailOnHashMismatch => {
                let actual_hash = WebcHash::sha256(image);
                if actual_hash != info.webc_sha256 {
                    Err(ImageHashMismatchError {
                        source: info.webc.to_string(),
                        actual_hash,
                        expected_hash: info.webc_sha256,
                    }
                    .into())
                } else {
                    Ok(())
                }
            }
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(%dist.webc, %dist.webc_sha256))]
    async fn download(&self, dist: &DistributionInfo) -> Result<Bytes, Error> {
        if dist.webc.scheme() == "file" {
            match crate::runtime::resolver::utils::file_path_from_url(&dist.webc) {
                Ok(path) => {
                    let bytes = crate::spawn_blocking({
                        let path = path.clone();
                        move || std::fs::read(path)
                    })
                    .await?
                    .with_context(|| format!("Unable to read \"{}\"", path.display()))?;

                    let bytes = bytes::Bytes::from(bytes);

                    Self::validate_hash(&bytes, self.hash_validation, dist).await?;

                    return Ok(bytes);
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
            headers: self.headers(&dist.webc),
            url: dist.webc.clone(),
            method: Method::GET,
            body: None,
            options: Default::default(),
        };

        tracing::debug!(%request.url, %request.method, "webc_package_download_start");
        tracing::trace!(?request.headers);

        let response = self.client.request(request).await?;

        tracing::trace!(
            %response.status,
            %response.redirected,
            ?response.headers,
            response.len=response.body.as_ref().map(|body| body.len()),
            "Received a response",
        );

        let url = &dist.webc;
        if !response.is_ok() {
            return Err(
                crate::runtime::resolver::utils::http_error(&response).context(format!(
                    "package download failed: GET request to \"{}\" failed with status {}",
                    url, response.status
                )),
            );
        }

        let body = response.body.context("package download failed")?;
        tracing::debug!(%url, "package_download_succeeded");

        let body = bytes::Bytes::from(body);

        Self::validate_hash(&body, self.hash_validation, dist).await?;

        Ok(body)
    }

    fn headers(&self, url: &Url) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Accept", "application/webc".parse().unwrap());
        headers.insert("User-Agent", USER_AGENT.parse().unwrap());

        if url.has_authority() {
            if let Some(token) = self.tokens.get(url.authority()) {
                let header = format!("Bearer {token}");
                match header.parse() {
                    Ok(header) => {
                        headers.insert(http::header::AUTHORIZATION, header);
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = &e as &dyn std::error::Error,
                            "An error occurred while parsing the authorization header",
                        );
                    }
                }
            }
        }

        headers
    }
}

impl Default for BuiltinPackageLoader {
    fn default() -> Self {
        BuiltinPackageLoader::new()
    }
}

#[async_trait::async_trait]
impl PackageLoader for BuiltinPackageLoader {
    #[tracing::instrument(
        level="debug",
        skip_all,
        fields(
            pkg=%summary.pkg.id,
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
            match cache
                .save_and_load_as_mmapped(bytes.clone(), &summary.dist)
                .await
            {
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
                        pkg=%summary.pkg.id,
                        pkg.hash=%summary.dist.webc_sha256,
                        pkg.url=%summary.dist.webc,
                        "Unable to save the downloaded package to disk",
                    );
                }
            }
        }

        // The sad path - looks like we don't have a filesystem cache so we'll
        // need to keep the whole thing in memory.
        let container = crate::spawn_blocking(move || from_bytes(bytes)).await??;
        // We still want to cache it in memory, of course
        self.in_memory.save(&container, summary.dist.webc_sha256);
        Ok(container)
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        super::load_package_tree(root, self, resolution, root_is_local_dir).await
    }
}

#[derive(Clone, Debug)]
pub struct ImageHashMismatchError {
    source: String,
    expected_hash: WebcHash,
    actual_hash: WebcHash,
}

impl std::fmt::Display for ImageHashMismatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "image hash mismatch! expected hash '{}', but the computed hash is '{}' (source '{}')",
            self.expected_hash, self.actual_hash, self.source,
        )
    }
}

impl std::error::Error for ImageHashMismatchError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheValidationMode {
    /// Just emit a warning for all images where the filename doesn't match
    /// the expected hash.
    WarnOnMismatch,
    /// Remove images from the cache if the filename doesn't match the actual
    /// hash.
    PruneOnMismatch,
}

// FIXME: This implementation will block the async runtime and should use
// some sort of spawn_blocking() call to run it in the background.
#[derive(Debug)]
pub struct FileSystemCache {
    cache_dir: PathBuf,
}

impl FileSystemCache {
    const FILE_SUFFIX: &'static str = ".bin";

    fn temp_dir(&self) -> PathBuf {
        self.cache_dir.join("__temp__")
    }

    /// Validate that the cached image file names correspond to their actual
    /// file content hashes.
    fn validate_hashes(&self) -> Result<Vec<(PathBuf, ImageHashMismatchError)>, anyhow::Error> {
        let mut items = Vec::<(PathBuf, ImageHashMismatchError)>::new();

        let iter = match std::fs::read_dir(&self.cache_dir) {
            Ok(v) => v,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Cache dir does not exist, so nothing to validate.
                return Ok(Vec::new());
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Could not read image cache dir: '{}'",
                        self.cache_dir.display()
                    )
                });
            }
        };

        for res in iter {
            let entry = res?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            // Extract the hash from the filename.

            let hash_opt = entry
                .file_name()
                .to_str()
                .and_then(|x| {
                    let (raw_hash, _) = x.split_once(Self::FILE_SUFFIX)?;
                    Some(raw_hash)
                })
                .and_then(|x| WebcHash::parse_hex(x).ok());
            let Some(expected_hash) = hash_opt else {
                continue;
            };

            // Compute the actual hash.
            let path = entry.path();
            let actual_hash = WebcHash::for_file(&path)?;

            if actual_hash != expected_hash {
                let err = ImageHashMismatchError {
                    source: path.to_string_lossy().to_string(),
                    actual_hash,
                    expected_hash,
                };
                items.push((path, err));
            }
        }

        Ok(items)
    }

    async fn lookup(&self, hash: &WebcHash) -> Result<Option<Container>, Error> {
        let path = self.path(hash);

        let container = crate::spawn_blocking({
            let path = path.clone();
            move || from_disk(path)
        })
        .await?;
        match container {
            Ok(c) => Ok(Some(c)),
            Err(WasmerPackageError::ContainerError(ContainerError::Open { error, .. }))
            | Err(WasmerPackageError::ContainerError(ContainerError::Read { error, .. }))
            | Err(WasmerPackageError::ContainerError(ContainerError::Detect(DetectError::Io(
                error,
            )))) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => {
                let msg = format!("Unable to read \"{}\"", path.display());
                Err(Error::new(e).context(msg))
            }
        }
    }

    async fn save(&self, webc: Bytes, dist: &DistributionInfo) -> Result<PathBuf, Error> {
        let path = self.path(&dist.webc_sha256);
        let dist = dist.clone();
        let temp_dir = self.temp_dir();

        let path2 = path.clone();
        crate::spawn_blocking(move || {
            // Keep files in a temporary directory until they are fully written
            // to prevent temp files being included in [`Self::scan`] or `[Self::retain]`.

            std::fs::create_dir_all(&temp_dir)
                .with_context(|| format!("Unable to create directory '{}'", temp_dir.display()))?;

            let mut temp = NamedTempFile::new_in(&temp_dir)?;
            temp.write_all(&webc)?;
            temp.flush()?;
            temp.as_file_mut().sync_all()?;

            // Move the temporary file to the final location.
            temp.persist(&path)?;

            tracing::debug!(
                pkg.hash=%dist.webc_sha256,
                pkg.url=%dist.webc,
                path=%path.display(),
                num_bytes=webc.len(),
                "Saved to disk",
            );
            Result::<_, Error>::Ok(())
        })
        .await??;

        Ok(path2)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn save_and_load_as_mmapped(
        &self,
        webc: Bytes,
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
        filename.push_str(Self::FILE_SUFFIX);

        self.cache_dir.join(filename)
    }

    /// Scan all the cached webc files and invoke the callback for each.
    pub async fn scan<S, F>(&self, state: S, callback: F) -> Result<S, Error>
    where
        S: Send + 'static,
        F: Fn(&mut S, &std::fs::DirEntry) -> Result<(), Error> + Send + 'static,
    {
        let cache_dir = self.cache_dir.clone();
        tokio::task::spawn_blocking(move || -> Result<S, anyhow::Error> {
            let mut state = state;

            let iter = match std::fs::read_dir(&cache_dir) {
                Ok(v) => v,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // path does not exist, so nothing to scan.
                    return Ok(state);
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Could not read image cache dir: '{}'", cache_dir.display())
                    });
                }
            };

            for res in iter {
                let entry = res?;
                if !entry.file_type()?.is_file() {
                    continue;
                }

                callback(&mut state, &entry)?;
            }

            Ok(state)
        })
        .await?
        .context("tokio runtime failed")
    }

    /// Remove entries from the cache that do not pass the callback.
    pub async fn retain<S, F>(&self, state: S, filter: F) -> Result<S, Error>
    where
        S: Send + 'static,
        F: Fn(&mut S, &std::fs::DirEntry) -> Result<bool, anyhow::Error> + Send + 'static,
    {
        let cache_dir = self.cache_dir.clone();
        tokio::task::spawn_blocking(move || {
            let iter = match std::fs::read_dir(&cache_dir) {
                Ok(v) => v,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    // path does not exist, so nothing to scan.
                    return Ok(state);
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Could not read image cache dir: '{}'", cache_dir.display())
                    });
                }
            };

            let mut state = state;
            for res in iter {
                let entry = res?;
                if !entry.file_type()?.is_file() {
                    continue;
                }

                if !filter(&mut state, &entry)? {
                    tracing::debug!(
                        path=%entry.path().display(),
                        "Removing cached image file - does not pass the filter",
                    );
                    match std::fs::remove_file(entry.path()) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(fs_err) => {
                            tracing::warn!(
                                path=%entry.path().display(),
                                ?fs_err,
                                "Could not delete cached image file",
                            );
                        }
                    }
                }
            }

            Ok(state)
        })
        .await?
        .context("tokio runtime failed")
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
    use wasmer_config::package::PackageId;

    use crate::{
        http::{HttpRequest, HttpResponse},
        runtime::resolver::PackageInfo,
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

    async fn cache_misses_will_trigger_a_download_internal() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([HttpResponse {
            body: Some(PYTHON.to_vec()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        }]));
        let loader = BuiltinPackageLoader::new()
            .with_cache_dir(temp.path())
            .with_shared_http_client(client.clone());
        let summary = PackageSummary {
            pkg: PackageInfo {
                id: PackageId::new_named("python/python", "0.1.0".parse().unwrap()),
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

#[cfg(test)]
mod test {
    use super::*;

    // NOTE: must be a tokio test because the BuiltinPackageLoader::new()
    // constructor requires a runtime...
    #[tokio::test]
    async fn test_builtin_package_downloader_cache_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let contents = "fail";
        let correct_hash = WebcHash::sha256(contents);
        let used_hash =
            WebcHash::parse_hex("0000a28ea38a000f3a3328cb7fabe330638d3258affe1a869e3f92986222d997")
                .unwrap();
        let filename = format!("{}{}", used_hash, FileSystemCache::FILE_SUFFIX);
        let file_path = path.join(filename);
        std::fs::write(&file_path, contents).unwrap();

        let dl = BuiltinPackageLoader::new().with_cache_dir(path);

        let errors = dl
            .validate_cache(CacheValidationMode::PruneOnMismatch)
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].actual_hash, correct_hash);
        assert_eq!(errors[0].expected_hash, used_hash);

        assert_eq!(file_path.exists(), false);
    }

    #[tokio::test]
    async fn test_file_cache_scan_retain() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let cache = FileSystemCache {
            cache_dir: path.to_path_buf(),
        };

        {
            let state = cache
                .scan(0u64, |state: &mut u64, _entry| {
                    *state += 1;
                    Ok(())
                })
                .await
                .unwrap();

            assert_eq!(state, 0);
        }

        let path1 = cache
            .save(
                Bytes::from_static(b"test1"),
                &DistributionInfo {
                    webc: Url::parse("file:///test1.webc").unwrap(),
                    webc_sha256: WebcHash::sha256(b"test1"),
                },
            )
            .await
            .unwrap();
        let path2 = cache
            .save(
                Bytes::from_static(b"test2"),
                &DistributionInfo {
                    webc: Url::parse("file:///test2.webc").unwrap(),
                    webc_sha256: WebcHash::sha256(b"test2"),
                },
            )
            .await
            .unwrap();

        {
            let path1 = path1.clone();
            let path2 = path2.clone();
            let state = cache
                .scan(0u64, move |state: &mut u64, entry| {
                    *state += 1;
                    assert!(entry.path() == path1 || entry.path() == path2);
                    Ok(())
                })
                .await
                .unwrap();

            assert_eq!(state, 2);
        }

        {
            let path1 = path1.clone();
            let state = cache
                .retain(0u64, move |state: &mut u64, entry| {
                    *state += 1;
                    Ok(entry.path() == path1)
                })
                .await
                .unwrap();
            assert_eq!(state, 2);
        }

        assert!(path1.exists());
        assert!(!path2.exists(), "Path 2 should have been deleted");

        {
            let path1 = path1.clone();
            let state = cache
                .scan(0u64, move |state: &mut u64, entry| {
                    *state += 1;
                    assert!(entry.path() == path1);
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(state, 1);
        }
    }
}
