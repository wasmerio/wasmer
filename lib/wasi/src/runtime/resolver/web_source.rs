use std::{
    fmt::Write as _,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Error};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use url::Url;
use webc::compat::Container;

use crate::{
    http::{HttpClient, HttpRequest, HttpResponse, USER_AGENT},
    runtime::resolver::{
        DistributionInfo, PackageInfo, PackageSpecifier, Source, Summary, WebcHash,
    },
};

/// A [`Source`] which can query arbitrary packages on the internet.
///
/// # Implementation Notes
///
/// Unlike other [`Source`] implementations, this will need to download
/// a package if it is a [`PackageSpecifier::Url`]. Optionally, these downloaded
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

    /// Get the directory that is typically used when caching downloaded
    /// packages inside `$WASMER_DIR`.
    pub fn default_cache_dir(wasmer_dir: impl AsRef<Path>) -> PathBuf {
        wasmer_dir.as_ref().join("downloads")
    }

    /// Download a package and cache it locally.
    #[tracing::instrument(skip(self))]
    async fn get_locally_cached_file(&self, url: &Url) -> Result<PathBuf, Error> {
        // This function is a bit tricky because we go to great lengths to avoid
        // unnecessary downloads.

        let cache_key = sha256(url.as_str().as_bytes());

        // First, we figure out some basic information about the item
        let cache_info = CacheInfo::for_url(&cache_key, &self.cache_dir);

        // Next we check if we definitely got a cache hit
        let state = match classify_cache_using_mtime(cache_info, self.retry_period) {
            Ok(path) => {
                tracing::debug!(path=%path.display(), "Cache hit");
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
        self.atomically_save_file(&path, &bytes).await?;
        if let Some(etag) = etag {
            self.atomically_save_file(path.with_extension("etag"), etag.as_bytes())
                .await?;
        }

        Ok(path)
    }

    async fn atomically_save_file(&self, path: impl AsRef<Path>, data: &[u8]) -> Result<(), Error> {
        // FIXME: This will block the main thread
        let mut temp = NamedTempFile::new_in(&self.cache_dir)?;
        temp.write_all(data)?;
        temp.as_file().sync_all()?;
        temp.persist(path)?;

        Ok(())
    }

    async fn get_etag(&self, url: &Url) -> Result<String, Error> {
        let request = HttpRequest {
            url: url.to_string(),
            method: "HEAD".to_string(),
            headers: headers(),
            body: None,
            options: Default::default(),
        };
        let HttpResponse {
            ok,
            status,
            status_text,
            headers,
            ..
        } = self.client.request(request).await?;

        if !ok {
            anyhow::bail!("HEAD request to \"{url}\" failed with {status} {status_text}");
        }

        let etag = headers
            .into_iter()
            .find(|(name, _)| name.to_string().to_lowercase() == "etag")
            .map(|(_, value)| value)
            .context("The HEAD request didn't contain an ETag header`")?;

        Ok(etag)
    }

    async fn fetch(&self, url: &Url) -> Result<(Vec<u8>, Option<String>), Error> {
        let request = HttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: headers(),
            body: None,
            options: Default::default(),
        };
        let HttpResponse {
            ok,
            status,
            status_text,
            headers,
            body,
            ..
        } = self.client.request(request).await?;

        if !ok {
            anyhow::bail!("HEAD request to \"{url}\" failed with {status} {status_text}");
        }

        let body = body.context("Response didn't contain a body")?;

        let etag = headers
            .into_iter()
            .find(|(name, _)| name.to_string().to_lowercase() == "etag")
            .map(|(_, value)| value);

        Ok((body, etag))
    }
}

fn headers() -> Vec<(String, String)> {
    vec![
        ("Accept".to_string(), "application/webc".to_string()),
        ("User-Agent".to_string(), USER_AGENT.to_string()),
    ]
}

#[async_trait::async_trait]
impl Source for WebSource {
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        let url = match package {
            PackageSpecifier::Url(url) => url,
            _ => return Ok(Vec::new()),
        };

        let local_path = self.get_locally_cached_file(url).await?;

        // FIXME: this will block
        let webc_sha256 = WebcHash::for_file(&local_path)?;

        // Note: We want to use Container::from_disk() rather than the bytes
        // our HTTP client gave us because then we can use memory-mapped files
        let container = Container::from_disk(&local_path)?;
        let pkg = PackageInfo::from_manifest(container.manifest())?;
        let dist = DistributionInfo {
            webc: url.clone(),
            webc_sha256,
        };

        Ok(vec![Summary { pkg, dist }])
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
