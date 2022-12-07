//! Implements logic for parsing the source for a package / path / URL
//! used in `wasmer run`.

use crate::cli::WasmerCLIOptions;
use crate::commands::{RunWithPathBuf, RunWithoutFile};
use clap::CommandFactory;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::{fmt, str::FromStr};
use wasmer_registry::PartialWapmConfig;

/// Struct containing all combinations of file sources:
///
/// - URLs
/// - namespace/package
/// - command
/// - local file
///
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl ResolvedSplitVersion {
    /// Returns the local path for the installed package
    pub fn get_local_path(&self) -> Option<PathBuf> {
        wasmer_registry::get_local_package(
            self.registry.as_deref(),
            &self.package,
            self.version.as_deref(),
        )?
        .get_path()
        .ok()
    }
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
    /// Resolves the package / URL by querying the registry
    pub fn get_package_info(
        &self,
        registry: &str,
        debug: bool,
    ) -> Result<ResolvedSplitVersion, anyhow::Error> {
        let registry = wasmer_registry::format_graphql(registry);
        match &self.inner {
            SplitVersionInner::File { path } => Err(anyhow::anyhow!(
                "cannot extract package info from file path {path:?}"
            )),
            SplitVersionInner::Url(url) => {
                let manifest = wasmer_registry::get_remote_webc_manifest(url)
                    .map_err(|e| anyhow::anyhow!("error fetching {url}: {e}"))?;

                let package = manifest
                    .manifest
                    .package
                    .get("wapm")
                    .and_then(|w| match w {
                        serde_cbor::Value::Map(m) => Some(m),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("no package description for {url}: {manifest:#?}")
                    })?;

                Ok(ResolvedSplitVersion {
                    registry: None,
                    package: package
                        .get(&serde_cbor::Value::Text("name".to_string()))
                        .cloned()
                        .and_then(|s| match s {
                            serde_cbor::Value::Text(t) => Some(t),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            anyhow::anyhow!("no package name for {url}: {manifest:#?}")
                        })?,
                    version: package
                        .get(&serde_cbor::Value::Text("version".to_string()))
                        .cloned()
                        .and_then(|s| match s {
                            serde_cbor::Value::Text(t) => Some(t),
                            _ => None,
                        }),
                    command: manifest.manifest.entrypoint,
                })
            }
            SplitVersionInner::Package(p) => Ok(ResolvedSplitVersion {
                registry: Some(registry),
                package: p.package.clone(),
                version: p.version.as_ref().map(|f| f.to_string()),
                command: p.command.clone(),
            }),
            SplitVersionInner::Command(c) => {
                let mut sp = if debug {
                    crate::commands::start_spinner(format!("Looking up command {} ...", c.command))
                } else {
                    None
                };
                let result = wasmer_registry::query_command_from_registry(&registry, &c.command);
                if let Some(s) = sp.take() {
                    s.clear();
                }
                let _ = std::io::stdout().flush();

                // TODO: version is being ignored?
                let command = c.command.clone();
                if let Ok(o) = result {
                    Ok(ResolvedSplitVersion {
                        registry: Some(registry),
                        package: o.package.clone(),
                        version: Some(o.version),
                        command: Some(command),
                    })
                } else {
                    Err(anyhow::anyhow!(
                        "could not find {command} in registry {registry:?}"
                    ))
                }
            }
        }
    }

    /// Returns the run command
    pub fn get_run_command(
        &self,
        registry: Option<&str>,
        mut options: RunWithoutFile,
        debug: bool,
    ) -> Result<RunWithPathBuf, anyhow::Error> {
        match &self.inner {
            SplitVersionInner::File { path } => {
                return Ok(RunWithPathBuf {
                    path: Path::new(&path).to_path_buf(),
                    options,
                });
            }
            SplitVersionInner::Url(url) => {
                let checksum = wasmer_registry::get_remote_webc_checksum(url)
                    .map_err(|e| anyhow::anyhow!("error fetching {url}: {e}"))?;

                let packages = wasmer_registry::get_all_installed_webc_packages();

                if !packages.iter().any(|p| p.checksum == checksum) {
                    let sp = crate::commands::start_spinner(format!("Installing {}", url));

                    let result = wasmer_registry::install_webc_package(url, &checksum);

                    result.map_err(|e| anyhow::anyhow!("error fetching {url}: {e}"))?;

                    if let Some(sp) = sp {
                        sp.clear();
                    }
                }

                let webc_dir = wasmer_registry::get_webc_dir();

                let webc_install_path = webc_dir
                    .ok_or_else(|| anyhow::anyhow!("Error installing package: no webc dir"))?
                    .join(checksum);

                Ok(RunWithPathBuf {
                    path: webc_install_path,
                    options: options,
                })
            }
            _ => {
                let lookup_reg = match registry {
                    Some(s) => s.to_string(),
                    None => PartialWapmConfig::from_file()
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                        .registry
                        .get_current_registry(),
                };

                let package_info = self.get_package_info(&lookup_reg, debug)?;

                if let Some(p) = wasmer_registry::get_local_package(
                    registry,
                    &package_info.package,
                    package_info.version.as_deref(),
                ) {
                    if let Some(c) = package_info.command.as_ref() {
                        if options.command_name.is_none() {
                            options.command_name = Some(c.to_string());
                        }
                    }
                    let path = p
                        .get_path()
                        .map_err(|e| anyhow::anyhow!("no install dir for {p:?}: {e}"))?;
                    return Ok(RunWithPathBuf { path, options });
                }

                let mut sp = if debug {
                    crate::commands::start_spinner(format!(
                        "Installing package {} ...",
                        package_info.package
                    ))
                } else {
                    None
                };
                let result = wasmer_registry::install_package(
                    registry,
                    &package_info.package,
                    package_info.version.as_deref(),
                    None,
                    options.force_install,
                );
                if let Some(sp) = sp.take() {
                    sp.clear();
                }
                let _ = std::io::stdout().flush();

                match result {
                    Ok((_, package_dir)) => Ok(RunWithPathBuf {
                        path: package_dir,
                        options,
                    }),
                    Err(e) => Err(anyhow::anyhow!(
                        "could not install {}: {e}",
                        package_info.package
                    )),
                }
            }
        }
    }
}

/// Package specifier in the format of `package@version:command`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitVersionPackage {
    /// namespace/package
    pub package: String,
    /// version: latest | 1.0.2
    pub version: Option<ParsedPackageVersion>,
    /// :command
    pub command: Option<String>,
}

/// Command specifier in the format of `command@version`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitVersionCommand {
    /// Command of the `command`
    pub command: String,
    /// version: latest | 1.0.2
    pub version: Option<ParsedPackageVersion>,
}

/// What type of package should be run
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitVersionInner {
    /// Run a file
    File {
        /// Path to the file
        path: String,
    },
    /// Run a URL
    Url(url::Url),
    /// Run a package
    Package(SplitVersionPackage),
    /// Run a command
    Command(SplitVersionCommand),
}

/// Package version specifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedPackageVersion {
    /// version 1.0.2
    Version(semver::Version),
    /// version latest
    Latest,
}

impl fmt::Display for ParsedPackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParsedPackageVersion::Latest => write!(f, "latest"),
            ParsedPackageVersion::Version(v) => write!(f, "{}", v),
        }
    }
}

impl SplitVersionInner {
    /// Try parsing `s` as a URL
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

    /// Try parsing `s` as a package (`namespace/user`) or a file (`./namespace/user`)
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
            re3.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else if re4.is_match(s) {
            re4.captures(s)
                .map(|c| {
                    c.iter()
                        .flatten()
                        .map(|m| m.as_str().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            return Err(SplitVersionError::InvalidPackageName(format!(
                "Invalid package: {s:?}"
            )));
        };

        let namespace = match captures.get(1).cloned() {
            Some(s) => s,
            None => {
                return Err(SplitVersionError::InvalidPackageName(format!(
                    "Invalid package version: {s:?}: no namespace: {captures:?}"
                )))
            }
        };

        let name = match captures.get(2).cloned() {
            Some(s) => s,
            None => {
                return Err(SplitVersionError::InvalidPackageName(format!(
                    "Invalid package version: {s:?}: no name: {captures:?}"
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

        if namespace.is_empty() {
            return Err(SplitVersionError::InvalidPackageName(format!(
                "empty namespace {s:?}"
            )));
        }

        if !namespace
            .chars()
            .all(|c| char::is_alphanumeric(c) || c == '-' || c == '_')
        {
            return Err(SplitVersionError::InvalidPackageName(format!(
                "invalid characters in namespace {namespace:?}"
            )));
        }

        if !name
            .chars()
            .all(|c| char::is_alphanumeric(c) || c == '-' || c == '_')
        {
            return Err(SplitVersionError::InvalidPackageName(format!(
                "invalid characters in name {name:?}"
            )));
        }

        Ok(SplitVersionInner::Package(SplitVersionPackage {
            package: format!("{namespace}/{name}"),
            version,
            command: captures.get(if no_version { 3 } else { 4 }).cloned(),
        }))
    }

    /// Try parsing `s` as a command (`python@latest`) or a file (`python`)
    pub fn try_parse_command(s: &str) -> Result<Self, SplitVersionError> {
        let command = s.split('@').next().map(|s| s.to_string()).unwrap();
        if !command.chars().all(char::is_alphanumeric) {
            return Err(SplitVersionError::InvalidCommandName(s.to_string()));
        }
        let version = s.split('@').nth(1).map(|s| s.to_string());
        let version = match version.as_deref() {
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

    /// Try parsing `s` as a file (error if it doesn't exist)
    pub fn try_parse_file(s: &str) -> Result<Self, SplitVersionError> {
        let pathbuf = std::path::Path::new(s).to_path_buf();
        let canon = pathbuf.canonicalize().unwrap_or(pathbuf);
        if canon.exists() {
            if canon.is_file() || canon.join("wapm.toml").exists() {
                Ok(Self::File {
                    path: format!("{}", canon.display()),
                })
            } else {
                Err(SplitVersionError::InvalidDirectory(s.to_string()))
            }
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
    /// File $u is a directory
    InvalidDirectory(String),
}

impl std::error::Error for SplitVersionError {}

impl fmt::Display for SplitVersionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::SplitVersionError::*;
        match self {
            InvalidUrl(e) => write!(f, "parsing as URL: {e}"),
            InvalidCommandName(e) => write!(f, "parsing as command: {e}"),
            InvalidPackageName(p) => write!(f, "parsing as package: {p}"),
            FileDoesNotExist(path) => write!(f, "parsing as file: file does not exist: {path}"),
            InvalidDirectory(path) => write!(
                f,
                "parsing as file: file is a directory, but contains no wapm.toml: {path}"
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
        write!(f, "\r\n")?;
        for e in self.errors.iter() {
            write!(f, "    {}", e)?;
        }
        Ok(())
    }
}

impl std::error::Error for SplitVersionMultiError {}

impl SplitVersion {
    /// Parse a version from a String
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
            errors: vec![
                SplitVersionError::FileDoesNotExist(
                    "registry.wapm.io/graphql/python/python".to_string()
                ),
                SplitVersionError::InvalidUrl("relative URL without a base".to_string()),
                SplitVersionError::InvalidPackageName(
                    "invalid characters in namespace \"registry.wapm.io/graphql/python\""
                        .to_string()
                ),
                SplitVersionError::InvalidCommandName(
                    "registry.wapm.io/graphql/python/python".to_string()
                )
            ],
        }
    );
    assert_eq!(
        SplitVersion::parse("namespace/name@latest:command").unwrap(),
        SplitVersion {
            original: "namespace/name@latest:command".to_string(),
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
            original: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").to_string(),
            inner: SplitVersionInner::File {
                path: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").to_string()
            },
        }
    );
    assert_eq!(
        SplitVersion::parse(env!("CARGO_MANIFEST_DIR")).unwrap_err(),
        SplitVersionMultiError {
            original: env!("CARGO_MANIFEST_DIR").to_string(),
            errors: vec![
                SplitVersionError::InvalidDirectory(
                    "/Users/fs/Development/wasmer6/wasmer/lib/cli".to_string()
                ),
                SplitVersionError::InvalidUrl("relative URL without a base".to_string()),
                SplitVersionError::InvalidPackageName(
                    "invalid characters in namespace \"/Users/fs/Development/wasmer6/wasmer/lib\""
                        .to_string()
                ),
                SplitVersionError::InvalidCommandName(
                    "/Users/fs/Development/wasmer6/wasmer/lib/cli".to_string()
                ),
            ],
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
