//! Implements logic for parsing the source for a package / path / URL
//! used in `wasmer run`.

use crate::cli::WasmerCLIOptions;
use clap::CommandFactory;
use std::{fmt, str::FromStr};

/// Struct containing all combinations of file sources:
///
/// - URLs
/// - namespace/package
/// - command
/// - local file
///
#[derive(Debug, Clone, PartialEq)]
pub struct SplitVersion {
    /// Original string
    pub original: String,
    /// Type of source package / file path
    pub inner: SplitVersionInner,
}

impl fmt::Display for SplitVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl Default for SplitVersion {
    fn default() -> Self {
        SplitVersion {
            original: String::new(),
            inner: SplitVersionInner::Command(SplitVersionCommand {
                command: String::new(),
                version: None,
            }),
        }
    }
}

/// Resolved package, can be used to lookup the local package name
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSplitVersion {
    /// Registry this package belongs to (default = current active registry)
    pub registry: Option<String>,
    /// Package name
    pub package: String,
    /// Package version, default = latest
    pub version: Option<String>,
    /// Command to run (default = None / entrypoint)
    pub command: Option<String>,
}

impl fmt::Display for ResolvedSplitVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}@{}",
            self.package,
            self.version.as_deref().unwrap_or("latest")
        )
    }
}

impl SplitVersion {
    pub fn resolve(&self, _registry: Option<&str>) -> Result<ResolvedSplitVersion, anyhow::Error> {
        Err(anyhow::anyhow!("unimplemented"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitVersionPackage {
    pub package: String,
    pub version: Option<ParsedPackageVersion>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitVersionCommand {
    pub command: String,
    pub version: Option<ParsedPackageVersion>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SplitVersionInner {
    File { path: String },
    Url(url::Url),
    Package(SplitVersionPackage),
    Command(SplitVersionCommand),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedPackageVersion {
    Version(semver::Version),
    Latest,
}

impl SplitVersionInner {
    // Try parsing `s` as a URL
    pub fn try_parse_url(s: &str) -> Result<Self, SplitVersionError> {
        let url = url::Url::parse(s).map_err(|e| SplitVersionError::InvalidUrl(format!("{e}")))?;
        if !(url.scheme() == "http" || url.scheme() == "https") {
            return Err(SplitVersionError::InvalidUrl(format!(
                "invalid scheme {}",
                url.scheme()
            )));
        }
        Ok(Self::Url(url))
    }

    // Try parsing `s` as a package (`namespace/user`) or a file (`./namespace/user`)
    pub fn try_parse_package(s: &str) -> Result<Self, SplitVersionError> {
        let re1 = regex::Regex::new(r#"(.*)/(.*)@(.*):(.*)"#).unwrap();
        let re2 = regex::Regex::new(r#"(.*)/(.*)@(.*)"#).unwrap();
        let re3 = regex::Regex::new(r#"(.*)/(.*):(.*)"#).unwrap();
        let re4 = regex::Regex::new(r#"(.*)/(.*)"#).unwrap();

        let mut no_version = false;

        let captures = if re1.is_match(s) {
            re1.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re2.is_match(s) {
            re2.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re3.is_match(s) {
            no_version = true;
            re4.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re4.is_match(s) {
            re3.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            // maybe a command
            return Err(SplitVersionError::InvalidPackageName(format!(
                "Invalid package version: {s:?}"
            )));
        };

        let namespace = match captures.get(1).cloned() {
            Some(s) => s,
            None => {
                return Err(SplitVersionError::InvalidPackageName(format!(
                    "Invalid package version: {s:?}: no namespace"
                )))
            }
        };

        let name = match captures.get(2).cloned() {
            Some(s) => s,
            None => {
                return Err(SplitVersionError::InvalidPackageName(format!(
                    "Invalid package version: {s:?}: no name"
                )))
            }
        };

        let version = if no_version {
            None
        } else {
            match captures.get(3).as_ref().map(|s| s.as_str()) {
                Some("latest") => Some(ParsedPackageVersion::Latest),
                Some(s) => Some(ParsedPackageVersion::Version(
                    semver::Version::parse(s).map_err(|e| {
                        SplitVersionError::InvalidPackageName(format!(
                            "error parsing version {s}: {e}"
                        ))
                    })?,
                )),
                None => None,
            }
        };

        Ok(SplitVersionInner::Package(SplitVersionPackage {
            package: format!("{namespace}/{name}"),
            version,
            command: captures.get(if no_version { 3 } else { 4 }).cloned(),
        }))
    }

    // Try parsing `s` as a command (`python@latest`) or a file (`python`)
    pub fn try_parse_command(s: &str) -> Result<Self, SplitVersionError> {
        if !s.chars().all(|c| char::is_alphanumeric(c) || c == '@') {
            return Err(SplitVersionError::InvalidCommandName(s.to_string()));
        }
        let command = s.split('@').nth(0).map(|s| s.to_string()).unwrap();
        let version = s.split('@').nth(1).map(|s| s.to_string());
        let version = match version.as_ref().map(|s| s.as_str()) {
            Some("latest") => Some(ParsedPackageVersion::Latest),
            Some(s) => Some(ParsedPackageVersion::Version(
                semver::Version::parse(s)
                    .map_err(|e| SplitVersionError::InvalidCommandName(format!("{e}")))?,
            )),
            None => None,
        };

        let clap_commands = WasmerCLIOptions::command();
        let mut prohibited_package_names = clap_commands.get_subcommands().map(|s| s.get_name());

        if prohibited_package_names.any(|s| s == command.trim()) {
            return Err(SplitVersionError::InvalidCommandName(format!(
                "command name {command:?} is a built-in wasmer command"
            )));
        }

        Ok(Self::Command(SplitVersionCommand { command, version }))
    }

    // Try parsing `s` as a file (error if it doesn't exist)
    pub fn try_parse_file(s: &str) -> Result<Self, SplitVersionError> {
        let pathbuf = std::path::Path::new(s).to_path_buf();
        let canon = pathbuf.canonicalize().unwrap_or(pathbuf);
        if canon.exists() {
            Ok(Self::File {
                path: format!("{}", canon.display()),
            })
        } else {
            Err(SplitVersionError::FileDoesNotExist(s.to_string()))
        }
    }
}

/// Error that can happen when parsing a `SplitVersion`
#[derive(Debug, Clone, PartialEq, Hash, Eq, Ord, PartialOrd)]
pub enum SplitVersionError {
    /// Invalid URL $u
    InvalidUrl(String),
    /// Invalid command name (cannot contain : or /)
    InvalidCommandName(String),
    /// Invalid package name
    InvalidPackageName(String),
    /// File $u does not exist
    FileDoesNotExist(String),
}

impl std::error::Error for SplitVersionError {}

impl fmt::Display for SplitVersionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::SplitVersionError::*;
        match self {
            InvalidUrl(e) => write!(f, "error while parsing source as URL: {e}"),
            InvalidCommandName(e) => write!(f, "error while parsing source as command: {e}"),
            InvalidPackageName(p) => write!(f, "error while parsing source as package: {p}"),
            FileDoesNotExist(path) => write!(
                f,
                "error while trying to load source path: file does not exist: {path}"
            ),
        }
    }
}

/// Error(s) that can happen when parsing a `SplitVersion`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SplitVersionMultiError {
    /// Original string that was parsed, used to fixup wasmer run args
    pub original: String,
    /// Errors that happen when trying to resolve the package
    ///
    /// e.g.: error 1: tried to access filesystem, but file isn't there
    ///       error 2: tried to resolve package, but package doesn't exist
    ///       error 3: tried to find command, but command doesn't exist
    pub errors: Vec<SplitVersionError>,
}

impl fmt::Display for SplitVersionMultiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut err = anyhow::anyhow!("{}", self.original);
        for e in self.errors.iter() {
            err = err.context(e.clone());
        }
        err.fmt(f)
    }
}

impl std::error::Error for SplitVersionMultiError {}

impl SplitVersion {
    pub fn parse(s: &str) -> Result<SplitVersion, SplitVersionMultiError> {
        s.parse()
    }
}

impl FromStr for SplitVersion {
    type Err = SplitVersionMultiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut errors = Vec::new();

        match SplitVersionInner::try_parse_file(s) {
            Ok(o) => {
                return Ok(Self {
                    original: s.to_string(),
                    inner: o,
                })
            }
            Err(e) => {
                errors.push(e);
            }
        }

        match SplitVersionInner::try_parse_url(s) {
            Ok(o) => {
                return Ok(Self {
                    original: s.to_string(),
                    inner: o,
                })
            }
            Err(e) => {
                errors.push(e);
            }
        }

        match SplitVersionInner::try_parse_package(s) {
            Ok(o) => {
                return Ok(Self {
                    original: s.to_string(),
                    inner: o,
                })
            }
            Err(e) => {
                errors.push(e);
            }
        }

        match SplitVersionInner::try_parse_command(s) {
            Ok(o) => {
                return Ok(Self {
                    original: s.to_string(),
                    inner: o,
                })
            }
            Err(e) => {
                errors.push(e);
            }
        }

        Err(SplitVersionMultiError {
            original: s.to_string(),
            errors,
        })
    }
}

#[test]
fn test_split_version() {
    assert_eq!(
        SplitVersion::parse("registry.wapm.io/graphql/python/python").unwrap_err(),
        SplitVersionMultiError {
            original: "registry.wapm.io/graphql/python/python".to_string(),
            errors: vec![SplitVersionError::FileDoesNotExist(
                "registry.wapm.io/graphql/python/python".to_string()
            )],
        }
    );
    assert_eq!(
        SplitVersion::parse("namespace/name@latest:command").unwrap(),
        SplitVersion {
            original: "namespace/name@version:command".to_string(),
            inner: SplitVersionInner::Package(SplitVersionPackage {
                package: "namespace/name".to_string(),
                version: Some(ParsedPackageVersion::Latest),
                command: Some("command".to_string()),
            }),
        }
    );

    assert_eq!(
        SplitVersion::parse("namespace/name@1.0.2").unwrap(),
        SplitVersion {
            original: "namespace/name@1.0.2".to_string(),
            inner: SplitVersionInner::Package(SplitVersionPackage {
                package: "namespace/name".to_string(),
                version: Some(ParsedPackageVersion::Version(
                    semver::Version::parse("1.0.2").unwrap()
                )),
                command: None,
            }),
        }
    );

    assert_eq!(
        SplitVersion::parse("namespace/name").unwrap(),
        SplitVersion {
            original: "namespace/name".to_string(),
            inner: SplitVersionInner::Package(SplitVersionPackage {
                package: "namespace/name".to_string(),
                version: None,
                command: None,
            })
        }
    );
    assert_eq!(
        SplitVersion::parse(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap(),
        SplitVersion {
            original: "registry.wapm.io/namespace/name".to_string(),
            inner: SplitVersionInner::File {
                path: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").to_string()
            },
        }
    );
    assert_eq!(
        SplitVersion::parse(env!("CARGO_MANIFEST_DIR")).unwrap_err(),
        SplitVersionMultiError {
            original: env!("CARGO_MANIFEST_DIR").to_string(),
            errors: vec![SplitVersionError::FileDoesNotExist(
                env!("CARGO_MANIFEST_DIR").to_string()
            )],
        },
    );
    assert_eq!(
        SplitVersion::parse("python@latest").unwrap(),
        SplitVersion {
            original: "python@latest".to_string(),
            inner: SplitVersionInner::Command(SplitVersionCommand {
                command: "python".to_string(),
                version: Some(ParsedPackageVersion::Latest),
            })
        }
    );
}
