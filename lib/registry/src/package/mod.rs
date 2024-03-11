#[cfg(feature = "build-package")]
pub mod builder;

use crate::WasmerConfig;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::{fmt, str::FromStr};
use url::Url;

const REGEX_PACKAGE_WITH_VERSION: &str =
    r"^([a-zA-Z0-9\-_]+)/([a-zA-Z0-9\-_]+)(@([a-zA-Z0-9\.\-_]+*))?$";

lazy_static::lazy_static! {
    static ref PACKAGE_WITH_VERSION: Regex = regex::Regex::new(REGEX_PACKAGE_WITH_VERSION).unwrap();
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    pub namespace: String,
    pub name: String,
    pub version: Option<String>,
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.file())
    }
}

impl Package {
    /// Checks whether the package is already installed, if yes, returns the path to the root dir
    pub fn already_installed(&self, wasmer_dir: &Path) -> Option<PathBuf> {
        let checkouts_dir = crate::get_checkouts_dir(wasmer_dir);
        let config = WasmerConfig::from_file(wasmer_dir).ok()?;
        let current_registry = config.registry.get_current_registry();
        let hash = self.get_hash(&current_registry);

        let found = std::fs::read_dir(&checkouts_dir)
            .ok()?
            .filter_map(|e| Some(e.ok()?.file_name().to_str()?.to_string()))
            .find(|s| match self.version.as_ref() {
                None => s.contains(&hash),
                Some(v) => s.contains(&hash) && s.ends_with(v),
            })?;
        Some(checkouts_dir.join(found))
    }

    /// Checks if the URL is already installed, note that `{url}@{version}`
    /// and `{url}` are treated the same
    pub fn is_url_already_installed(url: &Url, wasmer_dir: &Path) -> Option<PathBuf> {
        let checkouts_dir = crate::get_checkouts_dir(wasmer_dir);

        let url_string = url.to_string();
        let (url, version) = match url_string.split('@').collect::<Vec<_>>()[..] {
            [url, version] => (url.to_string(), Some(version)),
            _ => (url_string, None),
        };
        let hash = Self::hash_url(&url);
        let found = std::fs::read_dir(&checkouts_dir)
            .ok()?
            .filter_map(|e| Some(e.ok()?.file_name().to_str()?.to_string()))
            .find(|s| match version.as_ref() {
                None => s.contains(&hash),
                Some(v) => s.contains(&hash) && s.ends_with(v),
            })?;
        Some(checkouts_dir.join(found))
    }

    /// Returns the hash of the URL with a maximum of 128 bytes length
    /// (necessary for not erroring on filesystem limitations)
    pub fn hash_url(url: &str) -> String {
        hex::encode(url).chars().take(128).collect()
    }

    /// Returns the hash of the URL with a maximum of 64 bytes length
    pub fn unhash_url(hashed: &str) -> String {
        String::from_utf8_lossy(&hex::decode(hashed).unwrap_or_default()).to_string()
    }

    /// Returns the hash of the package URL without the version
    /// (because the version is encoded as @version and isn't part of the hash itself)
    pub fn get_hash(&self, registry: &str) -> String {
        let url = self.get_url_without_version(registry);
        Self::hash_url(&url.unwrap_or_default())
    }

    fn get_url_without_version(&self, registry: &str) -> Result<String, anyhow::Error> {
        let url = self.url(registry);
        Ok(format!(
            "{}/{}/{}",
            url?.origin().ascii_serialization(),
            self.namespace,
            self.name
        ))
    }

    /// Returns the filename for this package
    pub fn file(&self) -> String {
        let version = self
            .version
            .as_ref()
            .map(|f| format!("@{f}"))
            .unwrap_or_default();
        format!("{}/{}{version}", self.namespace, self.name)
    }

    /// Returns the {namespace}/{name} package name
    pub fn package(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// Returns the full URL including the version for this package
    pub fn url(&self, registry: &str) -> Result<Url, anyhow::Error> {
        let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
            .extract(registry)
            .map_err(|e| anyhow::anyhow!("Invalid registry: {}: {e}", registry))?;

        let registry_tld = format!(
            "{}.{}",
            registry_tld.domain.as_deref().unwrap_or(""),
            registry_tld.suffix.as_deref().unwrap_or(""),
        );

        let version = self
            .version
            .as_ref()
            .map(|f| format!("@{f}"))
            .unwrap_or_default();
        let url = format!(
            "https://{registry_tld}/{}/{}{version}",
            self.namespace, self.name
        );
        url::Url::parse(&url).map_err(|e| anyhow::anyhow!("error parsing {url}: {e}"))
    }

    /// Returns the path to the installation directory.
    /// Does not check whether the installation directory already exists.
    pub fn get_path(&self, wasmer_dir: &Path) -> Result<PathBuf, anyhow::Error> {
        let checkouts_dir = crate::get_checkouts_dir(wasmer_dir);
        let config = WasmerConfig::from_file(wasmer_dir)
            .map_err(|e| anyhow::anyhow!("could not load config {e}"))?;
        let hash = self.get_hash(&config.registry.get_current_registry());

        match self.version.as_ref() {
            Some(v) => Ok(checkouts_dir.join(format!("{}@{}", hash, v))),
            None => Ok(checkouts_dir.join(&hash)),
        }
    }
}

impl FromStr for Package {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let captures = PACKAGE_WITH_VERSION
            .captures(s.trim())
            .map(|c| {
                c.iter()
                    .flatten()
                    .map(|m| m.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        match captures.len() {
            // namespace/package
            3 => {
                let namespace = captures[1].to_string();
                let name = captures[2].to_string();
                Ok(Package {
                    namespace,
                    name,
                    version: None,
                })
            }
            // namespace/package@version
            5 => {
                let namespace = captures[1].to_string();
                let name = captures[2].to_string();
                let version = captures[4].to_string();
                Ok(Package {
                    namespace,
                    name,
                    version: Some(version),
                })
            }
            other => Err(anyhow::anyhow!("invalid package {other}")),
        }
    }
}
