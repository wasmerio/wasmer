use std::{
    fmt::{self, Display, Formatter},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::Error;
use semver::VersionReq;
use sha2::{Digest, Sha256};
use url::Url;
use wasmer_config::package::{NamedPackageId, PackageHash, PackageId, PackageSource};
use webc::{
    metadata::{annotations::Wapm as WapmAnnotations, Manifest, UrlOrManifest},
    Container,
};

/// A dependency constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub alias: String,
    pub pkg: PackageSource,
}

impl Dependency {
    pub fn package_name(&self) -> Option<String> {
        self.pkg.as_named().map(|x| x.full_name())
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn version(&self) -> Option<&VersionReq> {
        self.pkg.as_named().and_then(|n| n.version_opt())
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
        self.pkg.id.clone()
    }

    pub fn from_webc_file(path: impl AsRef<Path>) -> Result<PackageSummary, Error> {
        let path = path.as_ref().canonicalize()?;
        let container = Container::from_disk(&path)?;
        let webc_sha256 = WebcHash::for_file(&path)?;
        let url = crate::runtime::resolver::utils::url_from_file_path(&path).ok_or_else(|| {
            anyhow::anyhow!("Unable to turn \"{}\" into a file:// URL", path.display())
        })?;

        let manifest = container.manifest();
        let id = PackageInfo::package_id_from_manifest(manifest)?
            .unwrap_or_else(|| PackageId::Hash(PackageHash::from_sha256_bytes(webc_sha256.0)));

        let pkg = PackageInfo::from_manifest(id, manifest, container.version())?;
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
    pub id: PackageId,
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
    pub fn package_ident_from_manifest(
        manifest: &Manifest,
    ) -> Result<Option<NamedPackageId>, Error> {
        let wapm_annotations = manifest.wapm()?;

        let name = wapm_annotations
            .as_ref()
            .map_or_else(|| None, |annotations| annotations.name.clone());

        let version = wapm_annotations.as_ref().map_or_else(
            || String::from("0.0.0"),
            |annotations| {
                annotations
                    .version
                    .clone()
                    .unwrap_or_else(|| String::from("0.0.0"))
            },
        );

        if let Some(name) = name {
            Ok(Some(NamedPackageId {
                full_name: name,
                version: version.parse()?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn package_id_from_manifest(
        manifest: &Manifest,
    ) -> Result<Option<PackageId>, anyhow::Error> {
        let ident = Self::package_ident_from_manifest(manifest)?;

        Ok(ident.map(PackageId::Named))
    }

    pub fn from_manifest(
        id: PackageId,
        manifest: &Manifest,
        webc_version: webc::Version,
    ) -> Result<Self, Error> {
        // FIXME: is this still needed?
        // let wapm_annotations = manifest.wapm()?;
        // let name = wapm_annotations
        //     .as_ref()
        //     .map_or_else(|| None, |annotations| annotations.name.clone());
        //
        // let version = wapm_annotations.as_ref().map_or_else(
        //     || String::from("0.0.0"),
        //     |annotations| {
        //         annotations
        //             .version
        //             .clone()
        //             .unwrap_or_else(|| String::from("0.0.0"))
        //     },
        // );

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

        let filesystem = filesystem_mapping_from_manifest(manifest, webc_version)?;

        Ok(PackageInfo {
            id,
            dependencies,
            commands,
            entrypoint: manifest.entrypoint.clone(),
            filesystem,
        })
    }

    pub fn id(&self) -> PackageId {
        self.id.clone()
    }
}

fn filesystem_mapping_from_manifest(
    manifest: &Manifest,
    webc_version: webc::Version,
) -> Result<Vec<FileSystemMapping>, serde_cbor::Error> {
    match manifest.filesystem()? {
        Some(webc::metadata::annotations::FileSystemMappings(mappings)) => {
            let mappings = mappings
                .into_iter()
                .map(|mapping| FileSystemMapping {
                    volume_name: mapping.volume_name,
                    mount_path: mapping.mount_path,
                    dependency_name: mapping.from,
                    original_path: mapping.host_path,
                })
                .collect();

            Ok(mappings)
        }
        None => {
            if webc_version == webc::Version::V2 || webc_version == webc::Version::V1 {
                tracing::debug!(
                    "No \"fs\" package annotations found. Mounting the \"atom\" volume to \"/\" for compatibility."
                );
                Ok(vec![FileSystemMapping {
                    volume_name: "atom".to_string(),
                    mount_path: "/".to_string(),
                    original_path: Some("/".to_string()),
                    dependency_name: None,
                }])
            } else {
                // There is no atom volume in v3 by default, so we return an empty Vec.
                Ok(vec![])
            }
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
    pub original_path: Option<String>,
    /// The name of the package this volume comes from (current package if
    /// `None`).
    pub dependency_name: Option<String>,
}

fn url_or_manifest_to_specifier(value: &UrlOrManifest) -> Result<PackageSource, Error> {
    match value {
        UrlOrManifest::Url(url) => Ok(PackageSource::Url(url.clone())),
        UrlOrManifest::Manifest(manifest) => {
            if let Ok(Some(WapmAnnotations { name, version, .. })) =
                manifest.package_annotation("wapm")
            {
                let version = version.unwrap().parse()?;
                let id = NamedPackageId {
                    full_name: name.unwrap(),
                    version,
                };

                return Ok(PackageSource::from(id));
            }

            if let Some(origin) = manifest
                .origin
                .as_deref()
                .and_then(|origin| Url::parse(origin).ok())
            {
                return Ok(PackageSource::Url(origin));
            }

            Err(Error::msg(
                "Unable to determine a package specifier for a vendored dependency",
            ))
        }
        UrlOrManifest::RegistryDependentUrl(specifier) => {
            specifier.parse().map_err(anyhow::Error::from)
        }
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
pub struct WebcHash(pub(crate) [u8; 32]);

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

    pub fn as_hex(&self) -> String {
        hex::encode(self.0)
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
