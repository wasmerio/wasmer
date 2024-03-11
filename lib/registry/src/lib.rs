//! High-level interactions with the Wasmer backend.
//!
//! The GraphQL schema can be updated by running `make` in the Wasmer repo's
//! root directory.
//!
//! ```console
//! $ make update-graphql-schema
//! curl -sSfL https://registry.wasmer.io/graphql/schema.graphql > lib/registry/graphql/schema.graphql
//! ```
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub mod api;
mod client;
pub mod config;
pub mod graphql;
pub mod interface;
pub mod login;
pub mod package;
pub mod publish;
pub mod subscriptions;
pub(crate) mod tokio_spawner;
pub mod types;
pub mod utils;
pub mod wasmer_env;

pub use crate::client::RegistryClient;

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use graphql_client::GraphQLQuery;
use tar::EntryType;

use crate::utils::normalize_path;
pub use crate::{
    config::{format_graphql, WasmerConfig},
    graphql::queries::get_bindings_query::ProgrammingLanguage,
    package::Package,
};

pub static PACKAGE_TOML_FILE_NAME: &str = "wasmer.toml";
pub static PACKAGE_TOML_FALLBACK_NAME: &str = "wapm.toml";
pub static GLOBAL_CONFIG_FILE_NAME: &str = "wasmer.toml";

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
#[non_exhaustive]
pub struct PackageDownloadInfo {
    pub registry: String,
    pub package: String,
    pub version: String,
    pub is_latest_version: bool,
    /// Is the package private?
    pub is_private: bool,
    pub commands: String,
    pub manifest: String,
    pub url: String,
    pub pirita_url: Option<String>,
}

pub fn query_command_from_registry(
    registry_url: &str,
    command_name: &str,
) -> Result<PackageDownloadInfo, String> {
    use crate::graphql::{
        execute_query,
        queries::{get_package_by_command_query, GetPackageByCommandQuery},
    };

    let q = GetPackageByCommandQuery::build_query(get_package_by_command_query::Variables {
        command_name: command_name.to_string(),
    });

    let response: get_package_by_command_query::ResponseData = execute_query(registry_url, "", &q)
        .map_err(|e| format!("Error sending GetPackageByCommandQuery:  {e}"))?;

    let command = response
        .get_command
        .ok_or_else(|| "GetPackageByCommandQuery: no get_command".to_string())?;

    let package = command.package_version.package.display_name;
    let version = command.package_version.version;
    let url = command.package_version.distribution.download_url;
    let pirita_url = command.package_version.distribution.pirita_download_url;

    Ok(PackageDownloadInfo {
        registry: registry_url.to_string(),
        package,
        version,
        is_latest_version: command.package_version.is_last_version,
        manifest: command.package_version.manifest,
        commands: command_name.to_string(),
        url,
        pirita_url,
        is_private: command.package_version.package.private,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub enum QueryPackageError {
    ErrorSendingQuery(String),
    NoPackageFound {
        name: String,
        version: Option<String>,
    },
}

impl fmt::Display for QueryPackageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            QueryPackageError::ErrorSendingQuery(q) => write!(f, "error sending query: {q}"),
            QueryPackageError::NoPackageFound { name, version } => {
                write!(f, "no package found for {name:?}")?;
                if let Some(version) = version {
                    write!(f, " (version = {version:?})")?;
                }

                Ok(())
            }
        }
    }
}

impl std::error::Error for QueryPackageError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub enum GetIfPackageHasNewVersionResult {
    // if version = Some(...) and the ~/.wasmer/checkouts/.../{version} exists, the package is already installed
    UseLocalAlreadyInstalled {
        registry_host: String,
        namespace: String,
        name: String,
        version: String,
        path: PathBuf,
    },
    // if version = None, check for the latest version
    LocalVersionMayBeOutdated {
        registry_host: String,
        namespace: String,
        name: String,
        /// Versions that are already installed + whether they are
        /// older (true) or younger (false) than the timeout
        installed_versions: Vec<(String, bool)>,
    },
    // registry / namespace / name / version doesn't exist yet
    PackageNotInstalledYet {
        registry_url: String,
        namespace: String,
        name: String,
        version: Option<String>,
    },
}

/// Returns the download info of the packages, on error returns all the available packages
/// i.e. (("foo/python", "wasmer.io"), ("bar/python" "wasmer.io")))
pub fn query_package_from_registry(
    registry_url: &str,
    name: &str,
    version: Option<&str>,
    auth_token: Option<&str>,
) -> Result<PackageDownloadInfo, QueryPackageError> {
    use crate::graphql::{
        execute_query,
        queries::{get_package_version_query, GetPackageVersionQuery},
    };

    let q = GetPackageVersionQuery::build_query(get_package_version_query::Variables {
        name: name.to_string(),
        version: version.map(|s| s.to_string()),
    });

    let response: get_package_version_query::ResponseData =
        execute_query(registry_url, auth_token.unwrap_or_default(), &q).map_err(|e| {
            QueryPackageError::ErrorSendingQuery(format!("Error sending GetPackagesQuery: {e}"))
        })?;

    let v = response
        .package_version
        .as_ref()
        .ok_or_else(|| QueryPackageError::NoPackageFound {
            name: name.to_string(),
            version: None,
        })?;

    let manifest = toml::from_str::<wasmer_toml::Manifest>(&v.manifest).map_err(|e| {
        QueryPackageError::ErrorSendingQuery(format!("Invalid manifest for crate {name:?}: {e}"))
    })?;

    Ok(PackageDownloadInfo {
        registry: registry_url.to_string(),
        package: v.package.name.clone(),

        version: v.version.clone(),
        is_latest_version: v.is_last_version,
        is_private: v.package.private,
        manifest: v.manifest.clone(),

        commands: manifest
            .commands
            .iter()
            .map(|s| s.get_name())
            .collect::<Vec<_>>()
            .join(", "),

        url: v.distribution.download_url.clone(),
        pirita_url: v.distribution.pirita_download_url.clone(),
    })
}

pub fn get_checkouts_dir(wasmer_dir: &Path) -> PathBuf {
    wasmer_dir.join("checkouts")
}

pub fn get_webc_dir(wasmer_dir: &Path) -> PathBuf {
    wasmer_dir.join("webc")
}

/// Convenience function that will unpack .tar.gz files and .tar.bz
/// files to a target directory (does NOT remove the original .tar.gz)
pub fn try_unpack_targz<P: AsRef<Path>>(
    target_targz_path: P,
    target_path: P,
    strip_toplevel: bool,
) -> Result<PathBuf, anyhow::Error> {
    let target_targz_path = target_targz_path.as_ref().to_string_lossy().to_string();
    let target_targz_path = crate::utils::normalize_path(&target_targz_path);
    let target_targz_path = Path::new(&target_targz_path);

    let target_path = target_path.as_ref().to_string_lossy().to_string();
    let target_path = crate::utils::normalize_path(&target_path);
    let target_path = Path::new(&target_path);

    let open_file = || {
        std::fs::File::open(target_targz_path)
            .map_err(|e| anyhow::anyhow!("failed to open {}: {e}", target_targz_path.display()))
    };

    let try_decode_gz = || {
        let file = open_file()?;
        let gz_decoded = flate2::read::GzDecoder::new(&file);
        let ar = tar::Archive::new(gz_decoded);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (gz_ar_unpack_sans_parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        } else {
            unpack_with_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (gz_ar_unpack) {}: {e}",
                    target_targz_path.display()
                )
            })
        }
    };

    let try_decode_xz = || {
        let file = open_file()?;
        let mut decomp: Vec<u8> = Vec::new();
        let mut bufread = std::io::BufReader::new(&file);
        lzma_rs::xz_decompress(&mut bufread, &mut decomp).map_err(|e| {
            anyhow::anyhow!(
                "failed to unpack (try_decode_xz) {}: {e}",
                target_targz_path.display()
            )
        })?;

        let cursor = std::io::Cursor::new(decomp);
        let mut ar = tar::Archive::new(cursor);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (sans parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        } else {
            ar.unpack(target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (with parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        }
    };

    try_decode_gz().or_else(|e1| {
        try_decode_xz()
            .map_err(|e2| anyhow::anyhow!("could not decode gz: {e1}, could not decode xz: {e2}"))
    })?;

    Ok(Path::new(&target_targz_path).to_path_buf())
}

/// Whether the top-level directory should be stripped
pub fn download_and_unpack_targz(
    url: &str,
    target_path: &Path,
    strip_toplevel: bool,
) -> Result<PathBuf, anyhow::Error> {
    let tempdir = tempfile::TempDir::new()?;

    let target_targz_path = tempdir.path().join("package.tar.gz");

    let mut resp = reqwest::blocking::get(url)
        .map_err(|e| anyhow::anyhow!("failed to download {url}: {e}"))?;

    {
        let mut file = std::fs::File::create(&target_targz_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to download {url} into {}: {e}",
                target_targz_path.display()
            )
        })?;

        resp.copy_to(&mut file)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    try_unpack_targz(target_targz_path.as_path(), target_path, strip_toplevel)
        .with_context(|| anyhow::anyhow!("Could not download {url}"))?;

    Ok(target_path.to_path_buf())
}

pub fn unpack_with_parent<R>(mut archive: tar::Archive<R>, dst: &Path) -> Result<(), anyhow::Error>
where
    R: std::io::Read,
{
    use std::path::Component::Normal;

    let dst_normalized = normalize_path(dst.to_string_lossy().as_ref());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path: PathBuf = entry
            .path()?
            .components()
            .skip(1) // strip top-level directory
            .filter(|c| matches!(c, Normal(_))) // prevent traversal attacks
            .collect();
        if entry.header().entry_type().is_file() {
            entry.unpack_in(&dst_normalized)?;
        } else if entry.header().entry_type() == EntryType::Directory {
            std::fs::create_dir_all(&Path::new(&dst_normalized).join(&path))?;
        }
    }
    Ok(())
}

pub fn unpack_sans_parent<R>(mut archive: tar::Archive<R>, dst: &Path) -> std::io::Result<()>
where
    R: std::io::Read,
{
    use std::path::Component::Normal;

    let dst_normalized = normalize_path(dst.to_string_lossy().as_ref());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path: PathBuf = entry
            .path()?
            .components()
            .skip(1) // strip top-level directory
            .filter(|c| matches!(c, Normal(_))) // prevent traversal attacks
            .collect();
        entry.unpack(Path::new(&dst_normalized).join(path))?;
    }
    Ok(())
}

pub fn whoami(
    wasmer_dir: &Path,
    registry: Option<&str>,
    token: Option<&str>,
) -> Result<(String, String), anyhow::Error> {
    use crate::graphql::queries::{who_am_i_query, WhoAmIQuery};

    let config = WasmerConfig::from_file(wasmer_dir);

    let config = config
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("{registry:?}"))?;

    let registry = match registry {
        Some(s) => format_graphql(s),
        None => config.registry.get_current_registry(),
    };

    let login_token = token
        .map(|s| s.to_string())
        .or_else(|| config.registry.get_login_token_for_registry(&registry))
        .ok_or_else(|| anyhow::anyhow!("not logged into registry {:?}", registry))?;

    let q = WhoAmIQuery::build_query(who_am_i_query::Variables {});
    let response: who_am_i_query::ResponseData =
        crate::graphql::execute_query(&registry, &login_token, &q)
            .with_context(|| format!("{registry:?}"))?;

    let username = response
        .viewer
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("not logged into registry {:?}", registry))?
        .username
        .to_string();

    Ok((registry, username))
}

pub fn test_if_registry_present(registry: &str) -> Result<bool, String> {
    use crate::graphql::queries::{test_if_registry_present, TestIfRegistryPresent};

    let q = TestIfRegistryPresent::build_query(test_if_registry_present::Variables {});
    crate::graphql::execute_query_modifier_inner_check_json(
        registry,
        "",
        &q,
        Some(Duration::from_secs(1)),
        |f| f,
    )
    .map_err(|e| format!("{e}"))?;

    Ok(true)
}

pub fn get_all_available_registries(wasmer_dir: &Path) -> Result<Vec<String>, String> {
    let config = WasmerConfig::from_file(wasmer_dir)?;
    let mut registries = Vec::new();
    for login in config.registry.tokens {
        registries.push(format_graphql(&login.registry));
    }
    Ok(registries)
}

/// A library that exposes bindings to a Wasmer package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    /// A unique ID specifying this set of bindings.
    pub id: String,
    /// The URL which can be used to download the files that were generated
    /// (typically as a `*.tar.gz` file).
    pub url: String,
    /// The programming language these bindings are written in.
    pub language: ProgrammingLanguage,
    /// The generator used to generate these bindings.
    pub generator: BindingsGenerator,
}

/// The generator used to create [`Bindings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingsGenerator {
    /// A unique ID specifying this generator.
    pub id: String,
    /// The generator package's name (e.g. `wasmer/wasmer-pack`).
    pub package_name: String,
    /// The exact package version.
    pub version: String,
    /// The name of the command that was used for generating bindings.
    pub command: String,
}

impl fmt::Display for BindingsGenerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let BindingsGenerator {
            package_name,
            version,
            command,
            ..
        } = self;

        write!(f, "{package_name}@{version}:{command}")?;

        Ok(())
    }
}

/// List all bindings associated with a particular package.
///
/// If a version number isn't provided, this will default to the most recently
/// published version.
pub fn list_bindings(
    registry: &str,
    name: &str,
    version: Option<&str>,
) -> Result<Vec<Bindings>, anyhow::Error> {
    use crate::graphql::queries::{
        get_bindings_query::{ResponseData, Variables},
        GetBindingsQuery,
    };

    let variables = Variables {
        name: name.to_string(),
        version: version.map(String::from),
    };

    let q = GetBindingsQuery::build_query(variables);
    let response: ResponseData = crate::graphql::execute_query(registry, "", &q)?;

    let package_version = response.package_version.context("Package not found")?;

    let mut bindings_packages = Vec::new();

    for b in package_version.bindings.into_iter().flatten() {
        let pkg = Bindings {
            id: b.id,
            url: b.url,
            language: b.language,
            generator: BindingsGenerator {
                id: b.generator.package_version.id,
                package_name: b.generator.package_version.package.name,
                version: b.generator.package_version.version,
                command: b.generator.command_name,
            },
        };
        bindings_packages.push(pkg);
    }

    Ok(bindings_packages)
}
