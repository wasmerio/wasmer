use std::{fmt::Display, path::PathBuf, str::FromStr};

use anyhow::Context;
use semver::{Version, VersionReq};
use url::Url;

use crate::runtime::resolver::{PackageId, SourceId};

/// A reference to *some* package somewhere that the user wants to run.
///
/// # Security Considerations
///
/// The [`PackageSpecifier::Path`] variant doesn't specify which filesystem a
/// [`Source`] will eventually query. Consumers of [`PackageSpecifier`] should
/// be wary of sandbox escapes.
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

impl FromStr for PackageSpecifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
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

        let version = version
            .parse()
            .with_context(|| format!("Invalid version number, \"{version}\""))?;

        Ok(PackageSpecifier::Registry {
            full_name: full_name.to_string(),
            version,
        })
    }
}

impl Display for PackageSpecifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSpecifier::Registry { full_name, version } => write!(f, "{full_name}@{version}"),
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

/// Some metadata a [`Source`] can provide about a package without needing
/// to download the entire `*.webc` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The package's full name (i.e. `wasmer/wapm2pirita`).
    pub package_name: String,
    /// The package version.
    pub version: Version,
    /// A URL that can be used to download the `*.webc` file.
    pub webc: Url,
    /// A SHA-256 checksum for the `*.webc` file.
    pub webc_sha256: [u8; 32],
    /// Any dependencies this package may have.
    pub dependencies: Vec<Dependency>,
    /// Commands this package exposes to the outside world.
    pub commands: Vec<Command>,
    /// The [`Source`] this [`Summary`] came from.
    pub source: SourceId,
}

impl Summary {
    pub fn package_id(&self) -> PackageId {
        PackageId {
            package_name: self.package_name.clone(),
            version: self.version.clone(),
            source: self.source.clone(),
        }
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
        ];

        for (src, expected) in inputs {
            let parsed = PackageSpecifier::from_str(src).unwrap();
            assert_eq!(parsed, expected);
        }
    }
}
