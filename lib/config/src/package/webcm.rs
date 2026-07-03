//! The `.webcm` sidecar manifest format.
//!
//! A built `.webc` is stripped of its package identity. A `.webcm` is a small
//! TOML file next to it (same basename, e.g. `phpix.webcm` beside `phpix.webc`)
//! that records that identity, making it the canonical metadata source for
//! local `.webc` artifacts:
//!
//! ```toml
//! format_version = 1
//!
//! [package]
//! name = "wasmer/phpix"
//! version = "0.1.2"
//! hash = "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c"
//! ```

use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use super::{NamedPackageId, PackageHash};

/// A parsed `.webcm` sidecar manifest describing the identity of a `.webc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Webcm {
    /// The version of the `.webcm` format itself, bumped only on breaking
    /// changes (unknown fields are ignored, so additive changes need no
    /// bump). Written as [`Webcm::FORMAT_VERSION`]; missing is read as `1`.
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    pub package: WebcmPackage,
}

/// The identity of the `.webc` a [`Webcm`] pairs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebcmPackage {
    /// The full package name, e.g. `wasmer/phpix`.
    pub name: String,
    pub version: Version,
    /// The package hash of the paired `.webc`. Optional so identity-only
    /// manifests can be written by hand; when present, tooling must reject a
    /// paired `.webc` that hashes differently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<PackageHash>,
}

fn default_format_version() -> u32 {
    Webcm::FORMAT_VERSION
}

impl Webcm {
    /// The current `.webcm` format version.
    pub const FORMAT_VERSION: u32 = 1;

    /// The file extension of sidecar manifests, without the dot.
    pub const EXTENSION: &'static str = "webcm";

    pub fn new(id: NamedPackageId, hash: Option<PackageHash>) -> Self {
        Webcm {
            format_version: Self::FORMAT_VERSION,
            package: WebcmPackage {
                name: id.full_name,
                version: id.version,
                hash,
            },
        }
    }

    /// The package id recorded in this manifest.
    pub fn id(&self) -> NamedPackageId {
        NamedPackageId {
            full_name: self.package.name.clone(),
            version: self.package.version.clone(),
        }
    }

    /// The path of the `.webcm` pairing with `webc` (same basename, swapped
    /// extension).
    pub fn path_for_webc(webc: impl AsRef<Path>) -> PathBuf {
        webc.as_ref().with_extension(Self::EXTENSION)
    }

    /// The path of the `.webc` pairing with `webcm` (same basename, swapped
    /// extension).
    pub fn webc_path(webcm: impl AsRef<Path>) -> PathBuf {
        webcm.as_ref().with_extension("webc")
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

impl std::str::FromStr for Webcm {
    type Err = WebcmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let webcm: Webcm = toml::from_str(s)?;
        if webcm.format_version != Self::FORMAT_VERSION {
            return Err(WebcmError::UnsupportedFormatVersion {
                found: webcm.format_version,
            });
        }
        Ok(webcm)
    }
}

/// An error parsing a [`Webcm`].
#[derive(Debug, thiserror::Error)]
pub enum WebcmError {
    #[error("invalid webcm")]
    Toml(#[from] toml::de::Error),
    #[error(
        "unsupported webcm format version {found} (supported: {})",
        Webcm::FORMAT_VERSION
    )]
    UnsupportedFormatVersion { found: u32 },
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn parse_full_webcm() {
        let webcm = Webcm::from_str(
            r#"
format_version = 1

[package]
name = "wasmer/phpix"
version = "0.1.2"
hash = "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c"
"#,
        )
        .unwrap();

        assert_eq!(
            webcm.id(),
            NamedPackageId::try_new("wasmer/phpix", "0.1.2").unwrap()
        );
        assert_eq!(
            webcm.package.hash.unwrap().to_string(),
            "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c"
        );
    }

    #[test]
    fn format_version_and_hash_are_optional() {
        let webcm = Webcm::from_str("[package]\nname = \"phpix\"\nversion = \"0.1.2\"").unwrap();

        assert_eq!(webcm.format_version, Webcm::FORMAT_VERSION);
        assert_eq!(webcm.package.hash, None);
    }

    #[test]
    fn reject_unsupported_format_version() {
        let err = Webcm::from_str(
            "format_version = 99\n[package]\nname = \"phpix\"\nversion = \"0.1.2\"",
        )
        .unwrap_err();

        assert!(matches!(
            err,
            WebcmError::UnsupportedFormatVersion { found: 99 }
        ));
    }

    #[test]
    fn reject_malformed_webcm() {
        // Identity is the whole point; a manifest without it is invalid.
        assert!(Webcm::from_str("[package]\nname = \"phpix\"").is_err());
        assert!(Webcm::from_str("[package]\nname = \"phpix\"\nversion = \"not.semver\"").is_err());
        assert!(
            Webcm::from_str("[package]\nname = \"p\"\nversion = \"1.0.0\"\nhash = \"deadbeef\"")
                .is_err(),
            "hashes must carry the sha256: prefix"
        );
    }

    #[test]
    fn toml_roundtrip() {
        let webcm = Webcm::new(
            NamedPackageId::try_new("wasmer/phpix", "0.1.2").unwrap(),
            Some(
                "sha256:c355cd53795b9b481f7eb2b5f4f6c8cf73631bdc343723a579d671e32db70b3c"
                    .parse()
                    .unwrap(),
            ),
        );

        let toml = webcm.to_toml().unwrap();
        assert_eq!(Webcm::from_str(&toml).unwrap(), webcm);
    }

    #[test]
    fn path_pairing() {
        assert_eq!(
            Webcm::path_for_webc("pkgs/phpix.webc"),
            Path::new("pkgs/phpix.webcm")
        );
        assert_eq!(
            Webcm::webc_path("pkgs/phpix.webcm"),
            Path::new("pkgs/phpix.webc")
        );
    }
}
