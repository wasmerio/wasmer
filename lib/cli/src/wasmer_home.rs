#![allow(missing_docs)]

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Error};
use reqwest::{blocking::Client, Url};
use tempfile::NamedTempFile;
use wasmer::{AsEngineRef, DeserializeError, Module, SerializeError};
use wasmer_cache::Hash;
use wasmer_registry::Package;

const DEFAULT_REGISTRY: &str = "https://wapm.io/";
const CACHE_INVALIDATION_THRESHOLD: Duration = Duration::from_secs(5 * 60);

/// Something which can fetch resources from the internet and will cache them
/// locally.
pub trait DownloadCached {
    fn download_url(&self, url: &Url) -> Result<PathBuf, Error>;
    fn download_package(&self, pkg: &Package) -> Result<PathBuf, Error>;
}

#[derive(Debug, clap::Parser)]
pub struct WasmerHome {
    /// The Wasmer home directory.
    #[clap(long = "wasmer-dir", env = "WASMER_DIR")]
    pub home: Option<PathBuf>,
    /// Override the registry packages are downloaded from.
    #[clap(long, env = "WASMER_REGISTRY")]
    registry: Option<String>,
    /// Skip all caching.
    #[clap(long)]
    pub disable_cache: bool,
}

impl WasmerHome {
    pub fn wasmer_home(&self) -> Result<PathBuf, Error> {
        if let Some(wasmer_home) = &self.home {
            return Ok(wasmer_home.clone());
        }

        if let Some(user_home) = dirs::home_dir() {
            return Ok(user_home.join(".wasmer"));
        }

        anyhow::bail!("Unable to determine the Wasmer directory");
    }

    pub fn module_cache(&self) -> ModuleCache {
        if self.disable_cache {
            return ModuleCache::Disabled;
        };

        self.wasmer_home()
            .ok()
            .and_then(|home| wasmer_cache::FileSystemCache::new(home.join("cache")).ok())
            .map(ModuleCache::Enabled)
            .unwrap_or(ModuleCache::Disabled)
    }
}

impl DownloadCached for WasmerHome {
    #[tracing::instrument(skip_all)]
    fn download_url(&self, url: &Url) -> Result<PathBuf, Error> {
        tracing::debug!(%url, "Downloading");

        let home = self.wasmer_home()?;
        let checkouts = wasmer_registry::get_checkouts_dir(&home);

        // This function is a bit tricky because we go to great lengths to avoid
        // unnecessary downloads.

        let cache_key = Hash::generate(url.to_string().as_bytes());

        // First, we figure out some basic information about the item
        let cache_info = CacheInfo::for_url(&cache_key, &checkouts, self.disable_cache);

        // Next we check if we definitely got a cache hit
        let state = match classify_cache_using_mtime(cache_info) {
            Ok(path) => {
                tracing::debug!(path=%path.display(), "Cache hit");
                return Ok(path);
            }
            Err(s) => s,
        };

        // Okay, looks like we're going to have to download the item
        tracing::debug!(%url, "Sending a GET request");

        let client = Client::new();

        let request = client.get(url.clone()).header("Accept", "application/webc");

        let mut response = match request.send() {
            Ok(r) => r
                .error_for_status()
                .with_context(|| format!("The GET request to \"{url}\" was unsuccessful"))?,
            Err(e) => {
                // Something went wrong. If it was a connection issue and we've
                // got a cached file, let's use that and emit a warning.
                if e.is_connect() {
                    if let Some(path) = state.take_path() {
                        tracing::warn!(
                            path=%path.display(),
                            error=&e as &dyn std::error::Error,
                            "An error occurred while connecting to {}. Falling back to a cached version.",
                            url.host_str().unwrap_or(url.as_str()),
                        );
                        return Ok(path);
                    }
                }

                // Oh well, we tried.
                let msg = format!("Unable to send a GET request to \"{url}\"");
                return Err(Error::from(e).context(msg));
            }
        };

        tracing::debug!(
            status_code=%response.status(),
            url=%response.url(),
            content_length=response.content_length(),
            "Download started",
        );
        tracing::trace!(headers=?response.headers());

        // Now there is one last chance to avoid downloading the full file. If
        // it has an ETag header, we can use that to see whether the (possibly)
        // cached file is outdated.
        let etag = response
            .headers()
            .get("Etag")
            .and_then(|v| v.to_str().ok())
            .map(|etag| etag.trim().to_string());

        if let Some(cached) = state.use_etag_to_resolve_cached_file(etag.as_deref()) {
            tracing::debug!(
                path=%cached.display(),
                "Reusing the cached file because the ETag header is still valid",
            );
            return Ok(cached);
        }

        std::fs::create_dir_all(&checkouts)
            .with_context(|| format!("Unable to make sure \"{}\" exists", checkouts.display()))?;

        // Note: we want to copy directly into a file so we don't hold
        // everything in memory.
        let (mut f, path) = if self.disable_cache {
            // Leave the temporary file where it is. The OS will clean it up
            // for us later, and hopefully the caller will open it before the
            // temp file cleaner comes along.
            let temp = NamedTempFile::new().context("Unable to create a temporary file")?;
            temp.keep()
                .context("Unable to persist the temporary file")?
        } else {
            let cached_path = checkouts.join(cache_key.to_string());
            let f = std::fs::File::create(&cached_path).with_context(|| {
                format!("Unable to open \"{}\" for writing", cached_path.display())
            })?;

            (f, cached_path)
        };

        let bytes_read = std::io::copy(&mut response, &mut f)
            .and_then(|bytes_read| f.flush().map(|_| bytes_read))
            .with_context(|| format!("Unable to save the response to \"{}\"", path.display()))?;
        tracing::debug!(bytes_read, path=%path.display(), "Saved to disk");

        if !self.disable_cache {
            if let Some(etag) = etag {
                let etag_path = path.with_extension("etag");
                tracing::debug!(
                    path=%etag_path.display(),
                    %etag,
                    "Saving the ETag to disk",
                );

                if let Err(e) = std::fs::write(&etag_path, etag.as_bytes()) {
                    tracing::warn!(
                        error=&e as &dyn std::error::Error,
                        path=%etag_path.display(),
                        %etag,
                        "Unable to save the ETag to disk",
                    );
                }
            }
        }

        Ok(path)
    }

    fn download_package(&self, pkg: &Package) -> Result<PathBuf, Error> {
        let registry = self.registry.as_deref().unwrap_or(DEFAULT_REGISTRY);
        let url = package_url(registry, pkg)?;

        self.download_url(&url)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CacheInfo {
    /// Caching has been disabled.
    Disabled,
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
    fn for_url(key: &Hash, checkout_dir: &Path, disabled: bool) -> CacheInfo {
        if disabled {
            return CacheInfo::Disabled;
        }

        let path = checkout_dir.join(key.to_string());

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

fn classify_cache_using_mtime(info: CacheInfo) -> Result<PathBuf, CacheState> {
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
        CacheInfo::Disabled | CacheInfo::Miss { .. } => return Err(CacheState::Miss),
    };

    if let Ok(time_since_last_modified) = last_modified.elapsed() {
        if time_since_last_modified <= CACHE_INVALIDATION_THRESHOLD {
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

    fn use_etag_to_resolve_cached_file(self, new_etag: Option<&str>) -> Option<PathBuf> {
        match (new_etag, self) {
            (
                Some(new_etag),
                CacheState::PossiblyDirty {
                    etag: cached_etag,
                    path,
                },
            ) if cached_etag == new_etag => Some(path),
            _ => None,
        }
    }
}

fn package_url(registry: &str, pkg: &Package) -> Result<Url, Error> {
    let registry: Url = registry
        .parse()
        .with_context(|| format!("Unable to parse \"{registry}\" as a URL"))?;

    let Package {
        name,
        namespace,
        version,
    } = pkg;

    let mut path = format!("{namespace}/{name}");
    if let Some(version) = version {
        path.push('@');
        path.push_str(version);
    }

    let url = registry
        .join(&path)
        .context("Unable to construct the package URL")?;
    Ok(url)
}

#[derive(Debug, Clone)]
pub enum ModuleCache {
    Enabled(wasmer_cache::FileSystemCache),
    Disabled,
}

impl wasmer_cache::Cache for ModuleCache {
    type SerializeError = SerializeError;
    type DeserializeError = DeserializeError;

    unsafe fn load(
        &self,
        engine: &impl AsEngineRef,
        key: Hash,
    ) -> Result<Module, Self::DeserializeError> {
        match self {
            ModuleCache::Enabled(f) => f.load(engine, key),
            ModuleCache::Disabled => Err(DeserializeError::Io(std::io::ErrorKind::NotFound.into())),
        }
    }

    fn store(&mut self, key: Hash, module: &Module) -> Result<(), Self::SerializeError> {
        match self {
            ModuleCache::Enabled(f) => f.store(key, module),
            ModuleCache::Disabled => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construct_package_urls() {
        let inputs = [
            (
                "https://wapm.io/",
                "syrusakbary/python",
                "https://wapm.io/syrusakbary/python",
            ),
            (
                "https://wapm.dev",
                "syrusakbary/python@1.2.3",
                "https://wapm.dev/syrusakbary/python@1.2.3",
            ),
            (
                "https://localhost:8000/path/to/nested/dir/",
                "syrusakbary/python",
                "https://localhost:8000/path/to/nested/dir/syrusakbary/python",
            ),
        ];

        for (registry, package, expected) in inputs {
            let package: Package = package.parse().unwrap();

            let got = package_url(registry, &package).unwrap();
            assert_eq!(got.to_string(), expected);
        }
    }
}
