use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Error};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
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
/// Unlike other [`Source`] implementations, this will (by necessity) download
/// the package and cache it locally.
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
        Self {
            cache_dir: cache_dir.into(),
            client,
            retry_period: WebSource::DEFAULT_RETRY_PERIOD,
        }
    }

    /// Get the directory that is typically used when caching downloaded
    /// packages inside `$WASMER_DIR`.
    pub fn default_cache_dir(wasmer_dir: impl AsRef<Path>) -> PathBuf {
        wasmer_dir.as_ref().join("downloads")
    }

    /// Set the period after which an item should be marked as "possibly dirty"
    /// in the cache.
    pub fn with_retry_period(self, retry_period: Duration) -> Self {
        WebSource {
            retry_period,
            ..self
        }
    }
}

#[async_trait::async_trait]
impl Source for WebSource {
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        let url = match package {
            PackageSpecifier::Url(url) => url,
            _ => return Ok(Vec::new()),
        };

        let hash = sha256(url.as_str().as_bytes());
        let path = self.cache_dir.join(&hash).with_extension("bin");

        if path.exists() {
            todo!("Handle cache hits");
        }

        let request = HttpRequest {
            url: url.to_string(),
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
            anyhow::bail!("Request to \"{url}\" failed with {status} {status_text}");
        }

        let body = body.context("Response body was empty")?;

        // FIXME: We shouldn't block in async functions
        std::fs::create_dir_all(&self.cache_dir)?;
        let temp = NamedTempFile::new_in(&self.cache_dir)?;
        std::fs::write(&temp, &body)?;
        let path = self.cache_dir.join(&hash).with_extension("bin");
        temp.persist(&path)?;

        let container = Container::from_disk(&path)?;
        let pkg = PackageInfo::from_manifest(container.manifest())?;
        let dist = DistributionInfo {
            webc: url.clone(),
            webc_sha256: WebcHash::sha256(&body),
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
