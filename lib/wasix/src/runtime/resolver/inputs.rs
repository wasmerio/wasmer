use std::{
    fmt::{self, Display, Formatter},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Error};
use semver::{Version, VersionReq};
use sha2::{Digest, Sha256};
use url::Url;
use webc::{
    metadata::{annotations::Wapm as WapmAnnotations, Manifest, UrlOrManifest},
    Container,
};

use crate::runtime::resolver::PackageId;

/// A reference to *some* package somewhere that the user wants to run.
///
/// # Security Considerations
///
/// The [`PackageSpecifier::Path`] variant doesn't specify which filesystem a
/// [`Source`][source] will eventually query. Consumers of [`PackageSpecifier`]
/// should be wary of sandbox escapes.
///
/// [source]: crate::runtime::resolver::Source
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageSpecifier {
    Registry {
        full_name: String,
        version: VersionReq,
    },
    Url(Url),
    /// A `*.webc` file on disk.
    Path(PathBuf),
}

impl PackageSpecifier {
    pub fn parse(s: &str) -> Result<Self, anyhow::Error> {
        s.parse()
    }
}

impl FromStr for PackageSpecifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // There is no function in std for checking if a string is a valid path
        // and we can't do Path::new(s).exists() because that assumes the
        // package being specified is on the local filesystem, so let's make a
        // best-effort guess.
        if s.starts_with('.') || s.starts_with('/') {
            return Ok(PackageSpecifier::Path(s.into()));
        }
        #[cfg(windows)]
        if s.contains('\\') {
            return Ok(PackageSpecifier::Path(s.into()));
        }
        if Path::new(s).exists() {
            return Ok(PackageSpecifier::Path(s.into()));
        }

        if let Ok(url) = Url::parse(s) {
            if url.has_host() {
                return Ok(PackageSpecifier::Url(url));
            }
        }

        // TODO: Replace this with something more rigorous that can also handle
        // the locator field
        let (full_name, version) = match s.split_once('@') {
            Some((n, v)) => (n, v),
            None => (s, "*"),
        };

        let invalid_character = full_name
            .char_indices()
            .find(|(_, c)| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.'| '-'|'_' | '/'));
        if let Some((index, c)) = invalid_character {
            anyhow::bail!("Invalid character, {c:?}, at offset {index}");
        }

        let version = if version == "latest" {
            // let people write "some/package@latest"
            VersionReq::STAR
        } else {
            version
                .parse()
                .with_context(|| format!("Invalid version number, \"{version}\""))?
        };

        Ok(PackageSpecifier::Registry {
            full_name: full_name.to_string(),
            version,
        })
    }
}

impl Display for PackageSpecifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSpecifier::Registry { full_name, version } => {
                write!(f, "{full_name}")?;

                if !version.comparators.is_empty() {
                    write!(f, "@{version}")?;
                }

                Ok(())
            }
            PackageSpecifier::Url(url) => Display::fmt(url, f),
            PackageSpecifier::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

/// A dependency constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub alias: String,
    pub pkg: PackageSpecifier,
}

impl Dependency {
    pub fn package_name(&self) -> Option<&str> {
        match &self.pkg {
            PackageSpecifier::Registry { full_name, .. } => Some(full_name),
            _ => None,
        }
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn version(&self) -> Option<&VersionReq> {
        match &self.pkg {
            PackageSpecifier::Registry { version, .. } => Some(version),
            _ => None,
        }
    }
}

/// Some metadata a [`Source`][source] can provide about a package without
/// needing to download the entire `*.webc` file.
///
/// [source]: crate::runtime::resolver::Source
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSummary {
    pub pkg: PackageInfo,
    pub dist: DistributionInfo,
}

impl PackageSummary {
    pub fn package_id(&self) -> PackageId {
        self.pkg.id()
    }

    pub fn from_webc_file(path: impl AsRef<Path>) -> Result<PackageSummary, Error> {
        let path = path.as_ref().canonicalize()?;
        let container = Container::from_disk(&path)?;
        let webc_sha256 = WebcHash::for_file(&path)?;
        let url = crate::runtime::resolver::utils::url_from_file_path(&path).ok_or_else(|| {
            anyhow::anyhow!("Unable to turn \"{}\" into a file:// URL", path.display())
        })?;

        let pkg = PackageInfo::from_manifest(container.manifest())?;
        let dist = DistributionInfo {
            webc: url,
            webc_sha256,
        };

        Ok(PackageSummary { pkg, dist })
    }
}

/// Information about a package's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// The package's full name (i.e. `wasmer/wapm2pirita`).
    pub name: String,
    /// The package version.
    pub version: Version,
    /// Commands this package exposes to the outside world.
    pub commands: Vec<Command>,
    /// The name of a [`Command`] that should be used as this package's
    /// entrypoint.
    pub entrypoint: Option<String>,
    /// Any dependencies this package may have.
    pub dependencies: Vec<Dependency>,
    pub filesystem: Vec<FileSystemMapping>,
}

impl PackageInfo {
    pub fn from_manifest(manifest: &Manifest) -> Result<Self, Error> {
        let WapmAnnotations { name, version, .. } = manifest
            .wapm()?
            .context("Unable to find the \"wapm\" annotations")?;

        let dependencies = manifest
            .use_map
            .iter()
            .map(|(alias, value)| {
                Ok(Dependency {
                    alias: alias.clone(),
                    pkg: url_or_manifest_to_specifier(value)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let commands = manifest
            .commands
            .iter()
            .map(|(name, _value)| crate::runtime::resolver::Command {
                name: name.to_string(),
            })
            .collect();

        let filesystem = filesystem_mapping_from_manifest(manifest)?;

        Ok(PackageInfo {
            name,
            version: version.parse()?,
            dependencies,
            commands,
            entrypoint: manifest.entrypoint.clone(),
            filesystem,
        })
    }

    pub fn id(&self) -> PackageId {
        PackageId {
            package_name: self.name.clone(),
            version: self.version.clone(),
        }
    }
}

fn filesystem_mapping_from_manifest(
    manifest: &Manifest,
) -> Result<Vec<FileSystemMapping>, serde_cbor::Error> {
    match manifest.filesystem()? {
        Some(webc::metadata::annotations::FileSystemMappings(mappings)) => {
            let mappings = mappings
                .into_iter()
                .map(|mapping| FileSystemMapping {
                    volume_name: mapping.volume_name,
                    mount_path: mapping.mount_path,
                    dependency_name: mapping.from,
                    original_path: mapping.original_path,
                })
                .collect();

            Ok(mappings)
        }
        None => {
            // A "fs" annotation hasn't been attached to this package. This was
            // the case when *.webc files were generated by wapm2pirita version
            // 1.0.29 and earlier.
            //
            // To maintain compatibility with those older packages, we'll say
            // that the "atom" volume from the current package is mounted to "/"
            // and contains all files in the package.
            tracing::debug!(
                "No \"fs\" package annotations found. Mounting the \"atom\" volume to \"/\" for compatibility."
            );
            Ok(vec![FileSystemMapping {
                volume_name: "atom".to_string(),
                mount_path: "/".to_string(),
                original_path: "/".to_string(),
                dependency_name: None,
            }])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemMapping {
    /// The volume to be mounted.
    pub volume_name: String,
    /// Where the volume should be mounted within the resulting filesystem.
    pub mount_path: String,
    /// The path of the mapped item within its original volume.
    pub original_path: String,
    /// The name of the package this volume comes from (current package if
    /// `None`).
    pub dependency_name: Option<String>,
}

fn url_or_manifest_to_specifier(value: &UrlOrManifest) -> Result<PackageSpecifier, Error> {
    match value {
        UrlOrManifest::Url(url) => Ok(PackageSpecifier::Url(url.clone())),
        UrlOrManifest::Manifest(manifest) => {
            if let Ok(Some(WapmAnnotations { name, version, .. })) =
                manifest.package_annotation("wapm")
            {
                let version = version.parse()?;
                return Ok(PackageSpecifier::Registry {
                    full_name: name,
                    version,
                });
            }

            if let Some(origin) = manifest
                .origin
                .as_deref()
                .and_then(|origin| Url::parse(origin).ok())
            {
                return Ok(PackageSpecifier::Url(origin));
            }

            Err(Error::msg(
                "Unable to determine a package specifier for a vendored dependency",
            ))
        }
        UrlOrManifest::RegistryDependentUrl(specifier) => specifier.parse(),
    }
}

/// Information used when retrieving a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionInfo {
    /// A URL that can be used to download the `*.webc` file.
    pub webc: Url,
    /// A SHA-256 checksum for the `*.webc` file.
    pub webc_sha256: WebcHash,
}

/// The SHA-256 hash of a `*.webc` file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebcHash([u8; 32]);

impl WebcHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        WebcHash(bytes)
    }

    /// Parse a sha256 hash from a hex-encoded string.
    pub fn parse_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let mut hash = [0_u8; 32];
        hex::decode_to_slice(hex_str, &mut hash)?;
        Ok(Self(hash))
    }

    pub fn for_file(path: &PathBuf) -> Result<Self, std::io::Error> {
        // check for a hash at the file location
        let path_hash = path.join(".sha256");
        if let Ok(mut file) = File::open(&path_hash) {
            let mut hash = Vec::new();
            if let Ok(amt) = file.read_to_end(&mut hash) {
                if amt == 32 {
                    return Ok(WebcHash::from_bytes(hash[0..32].try_into().unwrap()));
                }
            }
        }

        // compute the hash
        let mut hasher = Sha256::default();
        let mut reader = BufReader::new(File::open(path)?);

        loop {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            hasher.update(buffer);
            let bytes_read = buffer.len();
            reader.consume(bytes_read);
        }

        let hash = hasher.finalize().into();

        // write the cache of the hash to the file system
        std::fs::write(path_hash, hash).ok();
        let hash = WebcHash::from_bytes(hash);

        Ok(hash)
    }

    /// Generate a new [`WebcHash`] based on the SHA-256 hash of some bytes.
    pub fn sha256(webc: impl AsRef<[u8]>) -> Self {
        let webc = webc.as_ref();

        let mut hasher = Sha256::default();
        hasher.update(webc);
        WebcHash::from_bytes(hasher.finalize().into())
    }

    pub fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for WebcHash {
    fn from(bytes: [u8; 32]) -> Self {
        WebcHash::from_bytes(bytes)
    }
}

impl Display for WebcHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02X}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn parse_some_package_specifiers() {
        let inputs = [
            (
                "first",
                PackageSpecifier::Registry {
                    full_name: "first".to_string(),
                    version: VersionReq::STAR,
                },
            ),
            (
                "namespace/package",
                PackageSpecifier::Registry {
                    full_name: "namespace/package".to_string(),
                    version: VersionReq::STAR,
                },
            ),
            (
                "namespace/package@1.0.0",
                PackageSpecifier::Registry {
                    full_name: "namespace/package".to_string(),
                    version: "1.0.0".parse().unwrap(),
                },
            ),
            (
                "namespace/package@latest",
                PackageSpecifier::Registry {
                    full_name: "namespace/package".to_string(),
                    version: VersionReq::STAR,
                },
            ),
            (
                "https://wapm/io/namespace/package@1.0.0",
                PackageSpecifier::Url("https://wapm/io/namespace/package@1.0.0".parse().unwrap()),
            ),
            (
                "/path/to/some/file.webc",
                PackageSpecifier::Path("/path/to/some/file.webc".into()),
            ),
            ("./file.webc", PackageSpecifier::Path("./file.webc".into())),
            #[cfg(windows)]
            (
                r"C:\Path\to\some\file.webc",
                PackageSpecifier::Path(r"C:\Path\to\some\file.webc".into()),
            ),
        ];

        for (src, expected) in inputs {
            let parsed = PackageSpecifier::from_str(src).unwrap();
            assert_eq!(parsed, expected);
        }
    }
}
