use crate::PartialWapmConfig;
use std::path::PathBuf;
use std::{fmt, str::FromStr};
use url::Url;

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
    pub fn already_installed(&self, #[cfg(test)] test_name: &str) -> Option<PathBuf> {
        #[cfg(not(test))]
        let checkouts_dir = crate::get_checkouts_dir()?;
        #[cfg(test)]
        let checkouts_dir = crate::get_checkouts_dir(test_name)?;

        #[cfg(not(test))]
        let hash = self.get_hash();
        #[cfg(test)]
        let hash = self.get_hash(test_name);

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
    pub fn is_url_already_installed(url: &Url, #[cfg(test)] test_name: &str) -> Option<PathBuf> {
        #[cfg(not(test))]
        let checkouts_dir = crate::get_checkouts_dir()?;
        #[cfg(test)]
        let checkouts_dir = crate::get_checkouts_dir(test_name)?;

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
    pub fn get_hash(&self, #[cfg(test)] test_name: &str) -> String {
        #[cfg(test)]
        let url = self.get_url_without_version(test_name);
        #[cfg(not(test))]
        let url = self.get_url_without_version();
        Self::hash_url(&url.unwrap_or_default())
    }

    fn get_url_without_version(
        &self,
        #[cfg(test)] test_name: &str,
    ) -> Result<String, anyhow::Error> {
        #[cfg(test)]
        let url = self.url(test_name);
        #[cfg(not(test))]
        let url = self.url();
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
    pub fn url(&self, #[cfg(test)] test_name: &str) -> Result<Url, anyhow::Error> {
        #[cfg(test)]
        let config = PartialWapmConfig::from_file(test_name)
            .map_err(|e| anyhow::anyhow!("could not read wapm config: {e}"))?;
        #[cfg(not(test))]
        let config = PartialWapmConfig::from_file()
            .map_err(|e| anyhow::anyhow!("could not read wapm config: {e}"))?;
        let registry = config.registry.get_current_registry();
        let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
            .extract(&registry)
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
    pub fn get_path(&self, #[cfg(test)] test_name: &str) -> Result<PathBuf, anyhow::Error> {
        #[cfg(test)]
        let checkouts_dir = crate::get_checkouts_dir(test_name);
        #[cfg(not(test))]
        let checkouts_dir = crate::get_checkouts_dir();

        let checkouts_dir = checkouts_dir.ok_or_else(|| anyhow::anyhow!("no checkouts dir"))?;

        #[cfg(not(test))]
        let hash = self.get_hash();
        #[cfg(test)]
        let hash = self.get_hash(test_name);

        match self.version.as_ref() {
            Some(v) => Ok(checkouts_dir.join(format!("{}@{}", hash, v))),
            None => Ok(checkouts_dir.join(&hash)),
        }
    }
}

impl FromStr for Package {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let regex =
            regex::Regex::new(r#"^([a-zA-Z0-9\-_]+)/([a-zA-Z0-9\-_]+)(@([a-zA-Z0-9\.\-_]+*))?$"#)
                .unwrap();

        let captures = regex
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
