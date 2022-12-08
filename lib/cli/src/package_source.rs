//! Implements logic for parsing the source for a package / path / URL
//! used in `wasmer run`.

use crate::commands::{RunWithPathBuf, RunWithoutFile};
use anyhow::Context;
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
pub struct PackageSource {
    /// Original string
    pub original: String,
    /// Type of source package / file path
    pub inner: PackageSourceInner,
}

impl FromStr for PackageSource {
    type Err = PackageSourceError;

    fn from_str(s: &str) -> Result<Self, PackageSourceError> {
        Ok(Self::parse(s))
    }
}

impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl Default for PackageSource {
    fn default() -> Self {
        PackageSource {
            original: String::new(),
            inner: PackageSourceInner::CommandOrFile(PackageSourceCommand {
                command: String::new(),
            }),
        }
    }
}

/// Resolved package, can be used to lookup the local package name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackageSource {
    /// Registry this package belongs to (default = current active registry)
    pub registry: Option<String>,
    /// Package name
    pub package: String,
    /// Package version, default = latest
    pub version: Option<String>,
    /// Command to run (default = None / entrypoint)
    pub command: Option<String>,
}

impl ResolvedPackageSource {
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

impl fmt::Display for ResolvedPackageSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}@{}",
            self.package,
            self.version.as_deref().unwrap_or("latest")
        )
    }
}

impl PackageSource {
    /// Resolves the package / URL by querying the registry
    pub fn get_package_info(
        &self,
        registry: &str,
        debug: bool,
    ) -> Result<ResolvedPackageSource, anyhow::Error> {
        let registry = wasmer_registry::format_graphql(registry);
        match &self.inner {
            // TODO: allow installing bindings from local directory with wapm.toml
            PackageSourceInner::File { path, .. } => Err(anyhow::anyhow!(
                "cannot extract package info from file path {path:?}"
            )),
            PackageSourceInner::Url(url) => {
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

                Ok(ResolvedPackageSource {
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
            PackageSourceInner::PackageOrFile(p) => Ok(ResolvedPackageSource {
                registry: Some(registry),
                package: p.package(),
                version: p.version.as_ref().map(|f| f.to_string()),
                command: None,
            }),
            PackageSourceInner::CommandOrFile(c) => {
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
                    Ok(ResolvedPackageSource {
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

    /// Returns the command name if self.inner == Command
    pub fn get_command_name(&self) -> Option<String> {
        if let PackageSourceInner::CommandOrFile(c) = &self.inner {
            Some(c.command.clone())
        } else {
            None
        }
    }

    /// Returns the run command
    pub fn get_run_command(
        &self,
        registry: Option<&str>,
        options: RunWithoutFile,
        debug: bool,
    ) -> Result<RunWithPathBuf, anyhow::Error> {
        let lookup_reg = match registry {
            Some(s) => s.to_string(),
            None => PartialWapmConfig::from_file()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .registry
                .get_current_registry(),
        };

        let mut run_with_path_buf = match self.get_url_or_file(&lookup_reg)? {
            PackageUrlOrFile::File(path) => RunWithPathBuf {
                path: Path::new(&path).to_path_buf(),
                options,
            },
            PackageUrlOrFile::Url(url) => {
                let mut sp = if debug {
                    crate::commands::start_spinner(format!("Installing {}", url))
                } else {
                    None
                };

                let (_, path) = wasmer_registry::install_package(&url).with_context(|| {
                    if let Some(sp) = sp.take() {
                        sp.clear();
                    }
                    let _ = std::io::stdout().flush();
                    anyhow::anyhow!("failed to install {url}")
                })?;

                if let Some(sp) = sp.take() {
                    sp.clear();
                }
                let _ = std::io::stdout().flush();

                RunWithPathBuf { path, options }
            }
        };

        if run_with_path_buf.options.command_name.is_none() {
            run_with_path_buf.options.command_name = self.get_command_name();
        }

        Ok(run_with_path_buf)
    }
}

/// Package specifier in the format of `package@version:command`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSourcePackage {
    /// namespace
    pub namespace: String,
    /// name
    pub name: String,
    /// version: latest | 1.0.2
    pub version: Option<ParsedPackageVersion>,
}

impl PackageSourcePackage {
    /// Returns the namespace/name formatted package name
    pub fn package(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

/// Command specifier in the format of `command@version`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSourceCommand {
    /// Command of the `command`
    pub command: String,
}

/// What type of package should be run
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSourceInner {
    /// Run a URL
    Url(url::Url),
    /// Run a package
    PackageOrFile(PackageSourcePackage),
    /// Run a command
    CommandOrFile(PackageSourceCommand),
    /// Run a file
    File {
        /// Path to the file
        path: String,
        /// Since "file" is a fallback in case no package name / command could be parsed,
        /// we track the errors that happened until here, so that we can print them if the
        /// file / command / package doesn't exist.
        errors: Vec<PackageSourceError>,
    },
}

/// Package version specifier
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedPackageVersion {
    /// version 1.0.2
    Version(semver::Version),
    /// version latest
    Latest,
}

impl ParsedPackageVersion {
    /// Parses a PackageVersion from a string (semver::Version | latest)
    pub fn parse(s: &str) -> Result<Self, PackageSourceError> {
        match s {
            "latest" => Ok(ParsedPackageVersion::Latest),
            s => Ok(ParsedPackageVersion::Version(
                semver::Version::parse(s).map_err(|e| {
                    PackageSourceError::InvalidPackageName(format!(
                        "error parsing version {s}: {e}"
                    ))
                })?,
            )),
        }
    }
}

impl fmt::Display for ParsedPackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParsedPackageVersion::Latest => write!(f, "latest"),
            ParsedPackageVersion::Version(v) => write!(f, "{}", v),
        }
    }
}

/// Resolved package source - file (if the file exists) or URL for fetching
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum PackageUrlOrFile {
    /// Existing file / directory to run
    File(PathBuf),
    /// Need to fetch the package / command from a remote registry
    Url(url::Url),
}

impl PackageSource {
    /// If the package source describes a local package / file, returns the file,
    /// otherwise returns the URL to fetch the package from.
    ///
    /// This function also checks if the package / command is already installed, if
    /// if is, it will return the directory path to the package root dir.
    pub fn get_url_or_file(&self, registry: &str) -> Result<PackageUrlOrFile, anyhow::Error> {
        let registry = wasmer_registry::format_graphql(registry);
        let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
            .extract(&registry)
            .map_err(|e| anyhow::anyhow!("Invalid registry: {}: {e}", registry))?;

        let registry_tld = format!(
            "{}.{}",
            registry_tld.domain.as_deref().unwrap_or(""),
            registry_tld.suffix.as_deref().unwrap_or(""),
        );

        match &self.inner {
            PackageSourceInner::File { path, errors } => {
                let pathbuf = Path::new(&path).to_path_buf();
                let canon = pathbuf.canonicalize().unwrap_or_else(|_| pathbuf.clone());
                let mut err = std::fs::metadata(&path)
                    .map(|_| PackageUrlOrFile::File(pathbuf))
                    .map_err(|e| anyhow::anyhow!("error opening path {:?}: {e}", canon.display()));

                for e in errors {
                    // if errors occurred, this will give context as to why the
                    // command / package was not recognized
                    err = err.with_context(|| anyhow::anyhow!("{e}"));
                }

                err.with_context(|| anyhow::anyhow!("{path}"))
            }
            PackageSourceInner::Url(url) => Ok(PackageUrlOrFile::Url(url.clone())),
            PackageSourceInner::PackageOrFile(package) => {
                // First test if the package name is a file
                let pathbuf = Path::new(&self.original).to_path_buf();
                if let Ok(file) =
                    std::fs::metadata(&self.original).map(|_| PackageUrlOrFile::File(pathbuf))
                {
                    return Ok(file);
                }

                // Check if the package is already installed or was recently installed
                let package_has_new_version = wasmer_registry::get_if_package_has_new_version(
                    &format!("https://{registry_tld}"),
                    &package.namespace,
                    &package.name,
                    package.version.as_ref().map(|s| s.to_string()),
                    std::time::Duration::from_secs(60 * 5),
                );

                if let Ok(
                    wasmer_registry::GetIfPackageHasNewVersionResult::UseLocalAlreadyInstalled {
                        registry_host,
                        namespace,
                        name,
                        version,
                        ..
                    },
                ) = package_has_new_version
                {
                    let local_package = wasmer_registry::LocalPackage {
                        registry: registry_host,
                        name: format!("{namespace}/{name}"),
                        version,
                    };
                    if let Ok(path) = local_package.get_path() {
                        return Ok(PackageUrlOrFile::File(path));
                    }
                }

                // else construct the URL to fetch the package from
                let version = package
                    .version
                    .as_ref()
                    .map(|v| format!("@{v}"))
                    .unwrap_or_default();

                let url = format!(
                    "https://{registry_tld}/{}/{}{version}",
                    package.namespace, package.name
                );

                Ok(PackageUrlOrFile::Url(url::Url::parse(&url).map_err(
                    |e| anyhow::anyhow!("Invalid package URL: {}: {e}", url),
                )?))
            }
            PackageSourceInner::CommandOrFile(command) => {
                // First test if the command name is a file
                let pathbuf = Path::new(&self.original).to_path_buf();
                if let Ok(file) =
                    std::fs::metadata(&self.original).map(|_| PackageUrlOrFile::File(pathbuf))
                {
                    return Ok(file);
                }

                // Check if command already installed
                if let Some(local_package) =
                    wasmer_registry::try_finding_local_command(&command.command)
                {
                    if let Ok(path) = local_package.get_path() {
                        return Ok(PackageUrlOrFile::File(path));
                    }
                }

                // else construct the URL to fetch the package from
                let url = format!("https://{registry_tld}/_/{}", command.command);
                Ok(PackageUrlOrFile::Url(url::Url::parse(&url).map_err(
                    |e| anyhow::anyhow!("Invalid package URL: {}: {e}", url),
                )?))
            }
        }
    }
}

impl PackageSourceInner {
    /// Try parsing `s` as a URL
    pub fn try_parse_url(s: &str) -> Result<Self, PackageSourceError> {
        let url = url::Url::parse(s).map_err(|e| PackageSourceError::InvalidUrl(format!("{e}")))?;
        if !(url.scheme() == "http" || url.scheme() == "https") {
            return Err(PackageSourceError::InvalidUrl(format!(
                "invalid scheme {}",
                url.scheme()
            )));
        }
        Ok(Self::Url(url))
    }

    /// Try parsing `s` as a package (`namespace/user`) or a file (`./namespace/user`)
    pub fn try_parse_package(s: &str) -> Result<Self, PackageSourceError> {
        let regex = regex::Regex::new(r#"^([a-zA-Z0-9\-_]+)/([a-zA-Z0-9\-_]+)(@(.*))?$"#).unwrap();

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
                let namespace = &captures[1];
                let name = &captures[2];
                Ok(Self::PackageOrFile(PackageSourcePackage {
                    name: name.to_string(),
                    namespace: namespace.to_string(),
                    version: None,
                }))
            }
            // namespace/package@version
            5 => {
                // captures.get(3).as_ref().map(|s| s.as_str())
                let namespace = &captures[1];
                let name = &captures[2];
                let version = &captures[4];
                Ok(Self::PackageOrFile(PackageSourcePackage {
                    name: name.to_string(),
                    namespace: namespace.to_string(),
                    version: Some(ParsedPackageVersion::parse(version)?),
                }))
            }
            _ => Err(PackageSourceError::InvalidPackageName(s.to_string())),
        }
    }

    /// Try parsing `s` as a command (`python@latest`) or a file (`python`)
    pub fn try_parse_command(s: &str) -> Result<Self, PackageSourceError> {
        if !s
            .chars()
            .all(|c| char::is_alphanumeric(c) || c == '-' || c == '_')
        {
            return Err(PackageSourceError::InvalidCommandName(s.to_string()));
        }
        Ok(Self::CommandOrFile(PackageSourceCommand {
            command: s.to_string(),
        }))
    }
}

/// Error that can happen when parsing a `PackageSource`
#[derive(Debug, Clone, PartialEq, Hash, Eq, Ord, PartialOrd)]
pub enum PackageSourceError {
    /// Invalid URL $u
    InvalidUrl(String),
    /// Invalid command name (cannot contain : or /)
    InvalidCommandName(String),
    /// Invalid version specifier (latest | 1.2.3)
    InvalidVersion(String),
    /// Invalid package name
    InvalidPackageName(String),
}

impl std::error::Error for PackageSourceError {}

impl fmt::Display for PackageSourceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::PackageSourceError::*;
        match self {
            InvalidUrl(e) => write!(f, "parsing as URL failed: {e}"),
            InvalidCommandName(e) => write!(f, "parsing as command failed: {e}"),
            InvalidPackageName(p) => write!(f, "parsing as package failed: {p}"),
            InvalidVersion(v) => write!(f, "invalid version {v}: use \"latest\" or 1.2.3"),
        }
    }
}

impl PackageSource {
    /// Parse a version from a String
    pub fn parse(s: &str) -> PackageSource {
        let mut errors = Vec::new();

        match PackageSourceInner::try_parse_url(s) {
            Ok(o) => {
                return Self {
                    original: s.to_string(),
                    inner: o,
                };
            }
            Err(e) => {
                errors.push(e);
            }
        }

        match PackageSourceInner::try_parse_package(s) {
            Ok(o) => {
                return Self {
                    original: s.to_string(),
                    inner: o,
                };
            }
            Err(e) => {
                errors.push(e);
            }
        }

        match PackageSourceInner::try_parse_command(s) {
            Ok(o) => {
                return Self {
                    original: s.to_string(),
                    inner: o,
                };
            }
            Err(e) => {
                errors.push(e);
            }
        }

        Self {
            original: s.to_string(),
            inner: PackageSourceInner::File {
                path: s.to_string(),
                errors,
            },
        }
    }
}

#[test]
fn test_package_source() {
    assert_eq!(
        PackageSource::parse("registry.wapm.io/graphql/python/python"),
        PackageSource {
            original: "registry.wapm.io/graphql/python/python".to_string(),
            inner: PackageSourceInner::File {
                path: "registry.wapm.io/graphql/python/python".to_string(),
                errors: vec![
                    PackageSourceError::InvalidUrl("relative URL without a base".to_string()),
                    PackageSourceError::InvalidPackageName(
                        "registry.wapm.io/graphql/python/python".to_string()
                    ),
                    PackageSourceError::InvalidCommandName(
                        "registry.wapm.io/graphql/python/python".to_string()
                    ),
                ],
            },
        }
    );

    assert_eq!(
        PackageSource::parse("/absolute/path/test.wasm"),
        PackageSource {
            original: "/absolute/path/test.wasm".to_string(),
            inner: PackageSourceInner::File {
                path: "/absolute/path/test.wasm".to_string(),
                errors: vec![
                    PackageSourceError::InvalidUrl("relative URL without a base".to_string()),
                    PackageSourceError::InvalidPackageName("/absolute/path/test.wasm".to_string()),
                    PackageSourceError::InvalidCommandName("/absolute/path/test.wasm".to_string()),
                ],
            },
        }
    );
    assert_eq!(
        PackageSource::parse("namespace/name@latest"),
        PackageSource {
            original: "namespace/name@latest".to_string(),
            inner: PackageSourceInner::PackageOrFile(PackageSourcePackage {
                namespace: "namespace".to_string(),
                name: "name".to_string(),
                version: Some(ParsedPackageVersion::Latest),
            }),
        }
    );

    assert_eq!(
        PackageSource::parse("namespace/name@latest:command"),
        PackageSource {
            original: "namespace/name@latest:command".to_string(),
            inner: PackageSourceInner::File {
                path: "namespace/name@latest:command".to_string(),
                errors: vec![
                    PackageSourceError::InvalidUrl("relative URL without a base".to_string()), 
                    PackageSourceError::InvalidPackageName("error parsing version latest:command: unexpected character 'l' while parsing major version number".to_string()), 
                    PackageSourceError::InvalidCommandName("namespace/name@latest:command".to_string()),
                ]
            },
        }
    );

    assert_eq!(
        PackageSource::parse("namespace/name@1.0.2"),
        PackageSource {
            original: "namespace/name@1.0.2".to_string(),
            inner: PackageSourceInner::PackageOrFile(PackageSourcePackage {
                namespace: "namespace".to_string(),
                name: "name".to_string(),
                version: Some(ParsedPackageVersion::Version(
                    semver::Version::parse("1.0.2").unwrap()
                )),
            }),
        }
    );

    assert_eq!(
        PackageSource::parse("namespace/name"),
        PackageSource {
            original: "namespace/name".to_string(),
            inner: PackageSourceInner::PackageOrFile(PackageSourcePackage {
                namespace: "namespace".to_string(),
                name: "name".to_string(),
                version: None,
            }),
        }
    );

    assert_eq!(
        PackageSource::parse("command"),
        PackageSource {
            original: "command".to_string(),
            inner: PackageSourceInner::CommandOrFile(PackageSourceCommand {
                command: "command".to_string(),
            }),
        }
    );

    assert_eq!(
        PackageSource::parse("https://wapm.io/syrusakbary/python"),
        PackageSource {
            original: "https://wapm.io/syrusakbary/python".to_string(),
            inner: PackageSourceInner::Url(
                url::Url::parse("https://wapm.io/syrusakbary/python").unwrap()
            ),
        }
    );

    assert_eq!(
        PackageSource::parse("python@latest"),
        PackageSource {
            original: "python@latest".to_string(),
            inner: PackageSourceInner::File {
                path: "python@latest".to_string(),
                errors: vec![
                    PackageSourceError::InvalidUrl("relative URL without a base".to_string()),
                    PackageSourceError::InvalidPackageName("python@latest".to_string()),
                    PackageSourceError::InvalidCommandName("python@latest".to_string()),
                ],
            }
        }
    );
}
