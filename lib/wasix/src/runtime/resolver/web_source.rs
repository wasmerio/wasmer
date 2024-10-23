use std::{
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Error};
use http::Method;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use url::Url;
use wasmer_config::package::{PackageHash, PackageId, PackageSource};
use wasmer_package::utils::from_disk;

use crate::{
    http::{HttpClient, HttpRequest},
    runtime::resolver::{
        DistributionInfo, PackageInfo, PackageSummary, QueryError, Source, WebcHash,
    },
};

/// A [`Source`] which can query arbitrary packages on the internet.
///
/// # Implementation Notes
///
/// Unlike other [`Source`] implementations, this will need to download
/// a package if it is a [`PackageSource::Url`]. Optionally, these downloaded
/// packages can be cached in a local directory.
///
/// After a certain period ([`WebSource::with_retry_period()`]), the
/// [`WebSource`] will re-check the uploaded source to make sure the cached
/// package is still valid. This checking is done using the [ETag][ETag] header,
/// if available.
///
/// [ETag]: https://en.wikipedia.org/wiki/HTTP_ETag
#[derive(Debug, Clone)]
pub struct WebSource {
    cache_dir: PathBuf,
    client: Arc<dyn HttpClient + Send + Sync>,
    retry_period: Duration,
}

impl WebSource {
    pub const DEFAULT_RETRY_PERIOD: Duration = Duration::from_secs(5 * 60);

    pub fn new(cache_dir: impl Into<PathBuf>, client: Arc<dyn HttpClient + Send + Sync>) -> Self {
        WebSource {
            cache_dir: cache_dir.into(),
            client,
            retry_period: WebSource::DEFAULT_RETRY_PERIOD,
        }
    }

    /// Set the period after which an item should be marked as "possibly dirty"
    /// in the cache.
    pub fn with_retry_period(self, retry_period: Duration) -> Self {
        WebSource {
            retry_period,
            ..self
        }
    }

    /// Download a package and cache it locally.
    #[tracing::instrument(level = "debug", skip_all, fields(%url))]
    async fn get_locally_cached_file(&self, url: &Url) -> Result<PathBuf, Error> {
        // This function is a bit tricky because we go to great lengths to avoid
        // unnecessary downloads.

        let cache_key = sha256(url.as_str().as_bytes());

        // First, we figure out some basic information about the item
        let cache_info = CacheInfo::for_url(&cache_key, &self.cache_dir);

        // Next we check if we definitely got a cache hit
        let state = match classify_cache_using_mtime(cache_info, self.retry_period) {
            Ok(path) => {
                tracing::debug!(path=%path.display(), "Cache hit!");
                return Ok(path);
            }
            Err(s) => s,
        };

        // Let's check if the ETag is still valid
        if let CacheState::PossiblyDirty { etag, path } = &state {
            match self.get_etag(url).await {
                Ok(new_etag) if new_etag == *etag => {
                    return Ok(path.clone());
                }
                Ok(different_etag) => {
                    tracing::debug!(
                        original_etag=%etag,
                        new_etag=%different_etag,
                        path=%path.display(),
                        "File has been updated. Redownloading.",
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        error=&*e,
                        path=%path.display(),
                        original_etag=%etag,
                        "Unable to check if the etag is out of date",
                    )
                }
            }
        }

        // Oh well, looks like we'll need to download it again
        let (bytes, etag) = match self.fetch(url).await {
            Ok((bytes, etag)) => (bytes, etag),
            Err(e) => {
                tracing::warn!(error = &*e, "Download failed");
                match state.take_path() {
                    Some(path) => {
                        tracing::debug!(
                            path=%path.display(),
                            "Using a possibly stale cached file",
                        );
                        return Ok(path);
                    }
                    None => {
                        return Err(e);
                    }
                }
            }
        };

        let path = self.cache_dir.join(&cache_key);
        self.atomically_save_file(&path, &bytes)
            .await
            .with_context(|| {
                format!(
                    "Unable to save the downloaded file to \"{}\"",
                    path.display()
                )
            })?;

        if let Some(etag) = etag {
            if let Err(e) = self
                .atomically_save_file(path.with_extension("etag"), etag.as_bytes())
                .await
            {
                tracing::warn!(
                    error=&*e,
                    %etag,
                    %url,
                    path=%path.display(),
                    "Unable to save the etag file",
                )
            }
        }

        Ok(path)
    }

    async fn atomically_save_file(&self, path: impl AsRef<Path>, data: &[u8]) -> Result<(), Error> {
        // FIXME: This will all block the main thread

        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Unable to create \"{}\"", parent.display()))?;
        }

        let mut temp = NamedTempFile::new_in(&self.cache_dir)?;
        temp.write_all(data)?;
        temp.as_file().sync_all()?;
        temp.persist(path)?;

        Ok(())
    }

    async fn get_etag(&self, url: &Url) -> Result<String, Error> {
        let request = HttpRequest {
            url: url.clone(),
            method: Method::HEAD,
            headers: super::utils::webc_headers(),
            body: None,
            options: Default::default(),
        };

        let response = self.client.request(request).await?;

        if !response.is_ok() {
            return Err(super::utils::http_error(&response)
                .context(format!("The HEAD request to \"{url}\" failed")));
        }

        let etag = response
            .headers
            .get("ETag")
            .context("The HEAD request didn't contain an ETag header`")?
            .to_str()
            .context("The ETag wasn't valid UTF-8")?;

        Ok(etag.to_string())
    }

    async fn fetch(&self, url: &Url) -> Result<(Vec<u8>, Option<String>), Error> {
        let request = HttpRequest {
            url: url.clone(),
            method: Method::GET,
            headers: super::utils::webc_headers(),
            body: None,
            options: Default::default(),
        };
        let response = self.client.request(request).await?;

        if !response.is_ok() {
            return Err(super::utils::http_error(&response)
                .context(format!("The GET request to \"{url}\" failed")));
        }

        let body = response.body.context("Response didn't contain a body")?;

        let etag = response
            .headers
            .get("ETag")
            .and_then(|etag| etag.to_str().ok())
            .map(|etag| etag.to_string());

        Ok((body, etag))
    }

    async fn load_url(&self, url: &Url) -> Result<Vec<PackageSummary>, anyhow::Error> {
        let local_path = self
            .get_locally_cached_file(url)
            .await
            .context("Unable to get the locally cached file")?;

        let webc_sha256 = crate::block_in_place(|| WebcHash::for_file(&local_path))
            .with_context(|| format!("Unable to hash \"{}\"", local_path.display()))?;

        // Note: We want to use Container::from_disk() rather than the bytes
        // our HTTP client gave us because then we can use memory-mapped files
        let container = crate::block_in_place(|| from_disk(&local_path))
            .with_context(|| format!("Unable to load \"{}\"", local_path.display()))?;

        let id = PackageInfo::package_id_from_manifest(container.manifest())?
            .unwrap_or_else(|| PackageId::Hash(PackageHash::from_sha256_bytes(webc_sha256.0)));

        let pkg = PackageInfo::from_manifest(id, container.manifest(), container.version())
            .context("Unable to determine the package's metadata")?;

        let dist = DistributionInfo {
            webc: url.clone(),
            webc_sha256,
        };

        Ok(vec![PackageSummary { pkg, dist }])
    }
}

#[async_trait::async_trait]
impl Source for WebSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        let url = match package {
            PackageSource::Url(url) => url,
            _ => {
                return Err(QueryError::Unsupported {
                    query: package.clone(),
                })
            }
        };

        self.load_url(url)
            .await
            .map_err(|error| QueryError::new_other(error, package))
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::default();
    hasher.update(bytes);
    let hash = hasher.finalize();
    let mut buffer = String::with_capacity(hash.len() * 2);
    for byte in hash {
        write!(buffer, "{byte:02X}").expect("Unreachable");
    }

    buffer
}

#[derive(Debug, Clone, PartialEq)]
enum CacheInfo {
    /// An item isn't in the cache, but could be cached later on.
    Miss,
    /// An item in the cache.
    Hit {
        path: PathBuf,
        etag: Option<String>,
        last_modified: Option<SystemTime>,
    },
}

impl CacheInfo {
    fn for_url(key: &str, checkout_dir: &Path) -> CacheInfo {
        let path = checkout_dir.join(key);

        if !path.exists() {
            return CacheInfo::Miss;
        }

        let etag = std::fs::read_to_string(path.with_extension("etag")).ok();
        let last_modified = path.metadata().and_then(|m| m.modified()).ok();

        CacheInfo::Hit {
            etag,
            last_modified,
            path,
        }
    }
}

fn classify_cache_using_mtime(
    info: CacheInfo,
    invalidation_threshold: Duration,
) -> Result<PathBuf, CacheState> {
    let (path, last_modified, etag) = match info {
        CacheInfo::Hit {
            path,
            last_modified: Some(last_modified),
            etag,
            ..
        } => (path, last_modified, etag),
        CacheInfo::Hit {
            path,
            last_modified: None,
            etag: Some(etag),
            ..
        } => return Err(CacheState::PossiblyDirty { etag, path }),
        CacheInfo::Hit {
            etag: None,
            last_modified: None,
            path,
            ..
        } => {
            return Err(CacheState::UnableToVerify { path });
        }
        CacheInfo::Miss { .. } => return Err(CacheState::Miss),
    };

    if let Ok(time_since_last_modified) = last_modified.elapsed() {
        if time_since_last_modified <= invalidation_threshold {
            return Ok(path);
        }
    }

    match etag {
        Some(etag) => Err(CacheState::PossiblyDirty { etag, path }),
        None => Err(CacheState::UnableToVerify { path }),
    }
}

/// Classification of how valid an item is based on filesystem metadata.
#[derive(Debug)]
enum CacheState {
    /// The item isn't in the cache.
    Miss,
    /// The cached item might be invalid, but it has an ETag we can use for
    /// further validation.
    PossiblyDirty { etag: String, path: PathBuf },
    /// The cached item exists on disk, but we weren't able to tell whether it is still
    /// valid, and there aren't any other ways to validate it further. You can
    /// probably reuse this if you are having internet issues.
    UnableToVerify { path: PathBuf },
}

impl CacheState {
    fn take_path(self) -> Option<PathBuf> {
        match self {
            CacheState::PossiblyDirty { path, .. } | CacheState::UnableToVerify { path } => {
                Some(path)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use futures::future::BoxFuture;
    use http::{header::IntoHeaderName, HeaderMap, StatusCode};
    use tempfile::TempDir;

    use crate::http::HttpResponse;

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../../c-api/examples/assets/python-0.1.0.wasmer");
    const COREUTILS: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc");
    const DUMMY_URL: &str = "http://my-registry.io/some/package";
    const DUMMY_URL_HASH: &str = "4D7481F44E1D971A8C60D3C7BD505E2727602CF9369ED623920E029C2BA2351D";

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

    struct ResponseBuilder(HttpResponse);

    impl ResponseBuilder {
        pub fn new() -> Self {
            ResponseBuilder(HttpResponse {
                body: None,
                redirected: false,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
            })
        }

        pub fn with_status(mut self, code: StatusCode) -> Self {
            self.0.status = code;
            self
        }

        pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
            self.0.body = Some(body.into());
            self
        }

        pub fn with_etag(self, value: &str) -> Self {
            self.with_header("ETag", value)
        }

        pub fn with_header(mut self, name: impl IntoHeaderName, value: &str) -> Self {
            self.0.headers.insert(name, value.parse().unwrap());
            self
        }

        pub fn build(self) -> HttpResponse {
            self.0
        }
    }

    async fn empty_cache_does_a_full_download_internal() {
        let dummy_etag = "This is an etag";
        let temp = TempDir::new().unwrap();
        let client = DummyClient::with_responses([ResponseBuilder::new()
            .with_body(PYTHON)
            .with_etag(dummy_etag)
            .build()]);
        let source = WebSource::new(temp.path(), Arc::new(client));
        let spec = PackageSource::Url(DUMMY_URL.parse().unwrap());

        let summaries = source.query(&spec).await.unwrap();

        // We got the right response, as expected
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pkg.id.as_named().unwrap().full_name, "python");
        // But we should have also cached the file and etag
        let path = temp.path().join(DUMMY_URL_HASH);
        assert!(path.exists());
        let etag_path = path.with_extension("etag");
        assert!(etag_path.exists());
        // And they should contain the correct content
        assert_eq!(std::fs::read_to_string(etag_path).unwrap(), dummy_etag);
        assert_eq!(std::fs::read(path).unwrap(), PYTHON);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn empty_cache_does_a_full_download() {
        empty_cache_does_a_full_download_internal().await
    }
    #[cfg(target_arch = "wasm32")]
    #[tokio::test()]
    async fn empty_cache_does_a_full_download() {
        empty_cache_does_a_full_download_internal().await
    }

    async fn cache_hit_internal() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([]));
        let source = WebSource::new(temp.path(), client.clone());
        let spec = PackageSource::Url(DUMMY_URL.parse().unwrap());
        // Prime the cache
        std::fs::write(temp.path().join(DUMMY_URL_HASH), PYTHON).unwrap();

        let summaries = source.query(&spec).await.unwrap();

        // We got the right response, as expected
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pkg.id.as_named().unwrap().full_name, "python");
        // And no requests were sent
        assert_eq!(client.requests.lock().unwrap().len(), 0);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn cache_hit() {
        cache_hit_internal().await
    }
    #[cfg(target_arch = "wasm32")]
    #[tokio::test()]
    async fn cache_hit() {
        cache_hit_internal().await
    }

    async fn fall_back_to_stale_cache_if_request_fails_internal() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([ResponseBuilder::new()
            .with_status(StatusCode::INTERNAL_SERVER_ERROR)
            .build()]));
        // Add something to the cache
        let python_path = temp.path().join(DUMMY_URL_HASH);
        std::fs::write(&python_path, PYTHON).unwrap();
        let source = WebSource::new(temp.path(), client.clone()).with_retry_period(Duration::ZERO);
        let spec = PackageSource::Url(DUMMY_URL.parse().unwrap());

        let summaries = source.query(&spec).await.unwrap();

        // We got the right response, as expected
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pkg.id.as_named().unwrap().full_name, "python");
        // And one request was sent
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        // The etag file wasn't written
        assert!(!python_path.with_extension("etag").exists());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn fall_back_to_stale_cache_if_request_fails() {
        fall_back_to_stale_cache_if_request_fails_internal().await
    }
    #[cfg(target_arch = "wasm32")]
    #[tokio::test()]
    async fn fall_back_to_stale_cache_if_request_fails() {
        fall_back_to_stale_cache_if_request_fails_internal().await
    }

    async fn download_again_if_etag_is_different_internal() {
        let temp = TempDir::new().unwrap();
        let client = Arc::new(DummyClient::with_responses([
            ResponseBuilder::new().with_etag("coreutils").build(),
            ResponseBuilder::new()
                .with_body(COREUTILS)
                .with_etag("coreutils")
                .build(),
        ]));
        // Add Python to the cache
        let path = temp.path().join(DUMMY_URL_HASH);
        std::fs::write(&path, PYTHON).unwrap();
        std::fs::write(path.with_extension("etag"), "python").unwrap();
        // but create a source that will always want to re-check the etags
        let source =
            WebSource::new(temp.path(), client.clone()).with_retry_period(Duration::new(0, 0));
        let spec = PackageSource::Url(DUMMY_URL.parse().unwrap());

        let summaries = source.query(&spec).await.unwrap();

        // Instead of Python (the originally cached item), we should get coreutils
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].pkg.id.as_named().unwrap().full_name,
            "sharrattj/coreutils"
        );
        // both a HEAD and GET request were sent
        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "HEAD");
        assert_eq!(requests[1].method, "GET");
        // The etag file was also updated
        assert_eq!(
            std::fs::read_to_string(path.with_extension("etag")).unwrap(),
            "coreutils"
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn download_again_if_etag_is_different() {
        download_again_if_etag_is_different_internal().await
    }
    #[cfg(target_arch = "wasm32")]
    #[tokio::test()]
    async fn download_again_if_etag_is_different() {
        download_again_if_etag_is_different_internal().await
    }
}
