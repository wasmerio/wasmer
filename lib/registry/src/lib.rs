//! High-level interactions with the WAPM backend.
//!
//! The GraphQL schema can be updated by running `make` in the Wasmer repo's
//! root directory.
//!
//! ```console
//! $ make update-graphql-schema
//! curl -sSfL https://registry.wapm.io/graphql/schema.graphql > lib/registry/graphql/schema.graphql
//! ```

pub mod api;
mod client;
pub mod config;
pub mod graphql;
pub mod interface;
pub mod login;
pub mod package;
pub mod publish;
pub mod types;
pub mod utils;

pub use client::RegistryClient;

use anyhow::Context;
use core::ops::Range;
use graphql_client::GraphQLQuery;
use reqwest::header::{ACCEPT, RANGE};
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tar::EntryType;
use url::Url;

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
pub struct PackageDownloadInfo {
    pub registry: String,
    pub package: String,
    pub version: String,
    pub is_latest_version: bool,
    pub commands: String,
    pub manifest: String,
    pub url: String,
    pub pirita_url: Option<String>,
}

pub fn get_package_local_dir(wasmer_dir: &Path, url: &str, version: &str) -> Option<PathBuf> {
    let checkouts_dir = get_checkouts_dir(wasmer_dir);
    let url_hash = Package::hash_url(url);
    let dir = checkouts_dir.join(format!("{url_hash}@{version}"));
    Some(dir)
}

pub fn try_finding_local_command(wasmer_dir: &Path, cmd: &str) -> Option<LocalPackage> {
    let local_packages = get_all_local_packages(wasmer_dir);
    for p in local_packages {
        let commands = p.get_commands();

        if commands.unwrap_or_default().iter().any(|c| c == cmd) {
            return Some(p);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct LocalPackage {
    pub registry: String,
    pub name: String,
    pub version: String,
    pub path: PathBuf,
}

impl fmt::Display for LocalPackage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}@{} (registry {}, located at {})",
            self.name,
            self.version,
            self.registry,
            self.path.display()
        )
    }
}

impl LocalPackage {
    pub fn get_path(&self) -> Result<PathBuf, String> {
        Ok(self.path.clone())
    }

    /// Returns the wasmer.toml path if it exists
    pub fn get_wasmer_toml_path(base_path: &Path) -> Result<PathBuf, anyhow::Error> {
        let path = base_path.join(PACKAGE_TOML_FILE_NAME);
        let fallback_path = base_path.join(PACKAGE_TOML_FALLBACK_NAME);
        if path.exists() {
            Ok(path)
        } else if fallback_path.exists() {
            Ok(fallback_path)
        } else {
            Err(anyhow::anyhow!(
                "neither {} nor {} exists",
                path.display(),
                fallback_path.display()
            ))
        }
    }

    /// Reads the wasmer.toml fron $PATH with wapm.toml as a fallback
    pub fn read_toml(base_path: &Path) -> Result<wasmer_toml::Manifest, String> {
        let wasmer_toml = std::fs::read_to_string(base_path.join(PACKAGE_TOML_FILE_NAME))
            .or_else(|_| std::fs::read_to_string(base_path.join(PACKAGE_TOML_FALLBACK_NAME)))
            .map_err(|_| {
                format!(
                    "Path {} has no {PACKAGE_TOML_FILE_NAME} or {PACKAGE_TOML_FALLBACK_NAME}",
                    base_path.display()
                )
            })?;
        let wasmer_toml = toml::from_str::<wasmer_toml::Manifest>(&wasmer_toml)
            .map_err(|e| format!("Could not parse toml for {:?}: {e}", base_path.display()))?;
        Ok(wasmer_toml)
    }

    pub fn get_commands(&self) -> Result<Vec<String>, String> {
        let path = self.get_path()?;
        let toml_parsed = Self::read_toml(&path)?;
        Ok(toml_parsed
            .command
            .unwrap_or_default()
            .iter()
            .map(|c| c.get_name())
            .collect())
    }
}

/// Returns the (manifest, .wasm file name), given a package dir
pub fn get_executable_file_from_path(
    package_dir: &Path,
    command: Option<&str>,
) -> Result<(wasmer_toml::Manifest, PathBuf), anyhow::Error> {
    let wasmer_toml = LocalPackage::read_toml(package_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let name = wasmer_toml.package.name.clone();
    let version = wasmer_toml.package.version.clone();

    let commands = wasmer_toml.command.clone().unwrap_or_default();
    let entrypoint_module = match command {
        Some(s) => commands.iter().find(|c| c.get_name() == s).ok_or_else(|| {
            anyhow::anyhow!("Cannot run {name}@{version}: package has no command {s:?}")
        })?,
        None => {
            if commands.is_empty() {
                Err(anyhow::anyhow!(
                    "Cannot run {name}@{version}: package has no commands"
                ))
            } else if commands.len() == 1 {
                Ok(&commands[0])
            } else {
                Err(anyhow::anyhow!("  -> wasmer run {name}@{version} --command-name={0}", commands.first().map(|f| f.get_name()).unwrap()))
                .context(anyhow::anyhow!("{}", commands.iter().map(|c| format!("`{}`", c.get_name())).collect::<Vec<_>>().join(", ")))
                .context(anyhow::anyhow!("You can run any of those by using the --command-name=COMMAND flag"))
                .context(anyhow::anyhow!("The `{name}@{version}` package doesn't have a default entrypoint, but has multiple available commands:"))
            }?
        }
    };

    let module_name = entrypoint_module.get_module();
    let modules = wasmer_toml.module.clone().unwrap_or_default();
    let entrypoint_module = modules
        .iter()
        .find(|m| m.name == module_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot run {name}@{version}: module {module_name} not found in {GLOBAL_CONFIG_FILE_NAME}"
            )
        })?;

    let entrypoint_source = package_dir.join(&entrypoint_module.source);

    Ok((wasmer_toml, entrypoint_source))
}

fn get_all_names_in_dir(dir: &PathBuf) -> Vec<(PathBuf, String)> {
    if !dir.is_dir() {
        return Vec::new();
    }

    let read_dir = match std::fs::read_dir(dir) {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let entries = read_dir
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, std::io::Error>>();

    let registry_entries = match entries {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    registry_entries
        .into_iter()
        .filter_map(|re| Some((re.clone(), re.file_name()?.to_str()?.to_string())))
        .collect()
}

/// Returns a list of all locally installed packages
pub fn get_all_local_packages(wasmer_dir: &Path) -> Vec<LocalPackage> {
    let mut packages = Vec::new();

    let checkouts_dir = get_checkouts_dir(wasmer_dir);

    for (path, url_hash_with_version) in get_all_names_in_dir(&checkouts_dir) {
        let manifest = match LocalPackage::read_toml(&path) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let url_hash = match url_hash_with_version.split('@').next() {
            Some(s) => s,
            None => continue,
        };
        let package =
            Url::parse(&Package::unhash_url(url_hash)).map(|s| s.origin().ascii_serialization());
        let host = match package {
            Ok(s) => s,
            Err(_) => continue,
        };
        packages.push(LocalPackage {
            registry: host,
            name: manifest.package.name,
            version: manifest.package.version.to_string(),
            path,
        });
    }

    packages
}

pub fn get_local_package(
    wasmer_dir: &Path,
    name: &str,
    version: Option<&str>,
) -> Option<LocalPackage> {
    get_all_local_packages(wasmer_dir)
        .iter()
        .find(|p| {
            if p.name != name {
                return false;
            }
            if let Some(v) = version {
                if p.version != v {
                    return false;
                }
            }
            true
        })
        .cloned()
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
                write!(f, "no package found for {name:?} (version = {version:?})")
            }
        }
    }
}

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
/// i.e. (("foo/python", "wapm.io"), ("bar/python" "wapm.io")))
pub fn query_package_from_registry(
    registry_url: &str,
    name: &str,
    version: Option<&str>,
) -> Result<PackageDownloadInfo, QueryPackageError> {
    use crate::graphql::{
        execute_query,
        queries::{get_package_version_query, GetPackageVersionQuery},
    };

    let q = GetPackageVersionQuery::build_query(get_package_version_query::Variables {
        name: name.to_string(),
        version: version.map(|s| s.to_string()),
    });

    let response: get_package_version_query::ResponseData = execute_query(registry_url, "", &q)
        .map_err(|e| {
            QueryPackageError::ErrorSendingQuery(format!("Error sending GetPackagesQuery: {e}"))
        })?;

    let v = response.package_version.as_ref().ok_or_else(|| {
        QueryPackageError::ErrorSendingQuery(format!("no package version for {name:?}"))
    })?;

    let manifest = toml::from_str::<wasmer_toml::Manifest>(&v.manifest).map_err(|e| {
        QueryPackageError::ErrorSendingQuery(format!("Invalid manifest for crate {name:?}: {e}"))
    })?;

    Ok(PackageDownloadInfo {
        registry: registry_url.to_string(),
        package: v.package.name.clone(),

        version: v.version.clone(),
        is_latest_version: v.is_last_version,
        manifest: v.manifest.clone(),

        commands: manifest
            .command
            .unwrap_or_default()
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
            ar.unpack(&target_path).map_err(|e| {
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
    let tempdir = tempdir::TempDir::new("wasmer-download-targz")?;

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

/// Installs the .tar.gz if it doesn't yet exist, returns the
/// (package dir, entrypoint .wasm file path)
pub fn install_package(wasmer_dir: &Path, url: &Url) -> Result<PathBuf, anyhow::Error> {
    use fs_extra::dir::copy;

    let tempdir = tempdir::TempDir::new("download")
        .map_err(|e| anyhow::anyhow!("could not create download temp dir: {e}"))?;

    let target_targz_path = tempdir.path().join("package.tar.gz");
    let unpacked_targz_path = tempdir.path().join("package");
    std::fs::create_dir_all(&unpacked_targz_path).map_err(|e| {
        anyhow::anyhow!(
            "could not create dir {}: {e}",
            unpacked_targz_path.display()
        )
    })?;

    get_targz_bytes(url, None, Some(target_targz_path.clone()))
        .map_err(|e| anyhow::anyhow!("failed to download {url}: {e}"))?;

    try_unpack_targz(
        target_targz_path.as_path(),
        unpacked_targz_path.as_path(),
        false,
    )
    .map_err(|e| anyhow::anyhow!("Could not unpack file downloaded from {url}: {e}"))?;

    // read {unpacked}/wasmer.toml to get the name + version number
    let toml_parsed = LocalPackage::read_toml(&unpacked_targz_path)
        .map_err(|e| anyhow::anyhow!("error reading package name / version number: {e}"))?;

    let version = toml_parsed.package.version.to_string();

    let checkouts_dir = crate::get_checkouts_dir(wasmer_dir);

    let installation_path =
        checkouts_dir.join(format!("{}@{version}", Package::hash_url(url.as_ref())));

    std::fs::create_dir_all(&installation_path)
        .map_err(|e| anyhow::anyhow!("could not create installation path for {url}: {e}"))?;

    let mut options = fs_extra::dir::CopyOptions::new();
    options.content_only = true;
    options.overwrite = true;
    copy(&unpacked_targz_path, &installation_path, &options)?;

    #[cfg(not(target_os = "wasi"))]
    let _ = filetime::set_file_mtime(
        LocalPackage::get_wasmer_toml_path(&installation_path)?,
        filetime::FileTime::now(),
    );

    Ok(installation_path)
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

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteWebcInfo {
    pub checksum: String,
    pub manifest: webc::Manifest,
}

pub fn install_webc_package(
    wasmer_dir: &Path,
    url: &Url,
    checksum: &str,
) -> Result<(), anyhow::Error> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { install_webc_package_inner(wasmer_dir, url, checksum).await })
}

async fn install_webc_package_inner(
    wasmer_dir: &Path,
    url: &Url,
    checksum: &str,
) -> Result<(), anyhow::Error> {
    use futures_util::StreamExt;

    let path = get_webc_dir(wasmer_dir);
    let _ = std::fs::create_dir_all(&path);
    let webc_path = path.join(checksum);

    let mut file = std::fs::File::create(&webc_path)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context(anyhow::anyhow!("{}", webc_path.display()))?;

    let client = {
        let builder = reqwest::Client::builder();
        let builder = crate::graphql::proxy::maybe_set_up_proxy(builder)?;
        builder
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("install_webc_package: failed to build reqwest Client")?
    };

    let res = client
        .get(url.clone())
        .header(ACCEPT, "application/webc")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context(anyhow::anyhow!("install_webc_package: failed to GET {url}"))?;

    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let item = item
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context(anyhow::anyhow!("install_webc_package: failed to GET {url}"))?;
        file.write_all(&item)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context(anyhow::anyhow!(
                "install_webc_package: failed to write chunk to {}",
                webc_path.display()
            ))?;
    }

    Ok(())
}

/// Returns a list of all installed webc packages
pub fn get_all_installed_webc_packages(wasmer_dir: &Path) -> Vec<RemoteWebcInfo> {
    get_all_installed_webc_packages_inner(wasmer_dir)
}

fn get_all_installed_webc_packages_inner(wasmer_dir: &Path) -> Vec<RemoteWebcInfo> {
    let dir = get_webc_dir(wasmer_dir);

    let read_dir = match std::fs::read_dir(dir) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    read_dir
        .filter_map(|r| Some(r.ok()?.path()))
        .filter_map(|path| {
            webc::WebCMmap::parse(
                path,
                &webc::ParseOptions {
                    parse_atoms: false,
                    parse_volumes: false,
                    ..Default::default()
                },
            )
            .ok()
        })
        .filter_map(|webc| {
            let checksum = webc.checksum.as_ref().map(|s| &s.data)?.to_vec();
            let hex_string = get_checksum_hash(&checksum);
            Some(RemoteWebcInfo {
                checksum: hex_string,
                manifest: webc.manifest.clone(),
            })
        })
        .collect()
}

/// The checksum of the webc file has a bunch of zeros at the end
/// (it's currently encoded that way in the webc format). This function
/// strips the zeros because otherwise the filename would become too long.
///
/// So:
///
/// `3ea47cb0000000000000` -> `3ea47cb`
///
pub fn get_checksum_hash(bytes: &[u8]) -> String {
    let mut checksum = bytes.to_vec();
    while checksum.last().copied() == Some(0) {
        checksum.pop();
    }
    hex::encode(&checksum).chars().take(64).collect()
}

/// Returns the checksum of the .webc file, so that we can check whether the
/// file is already installed before downloading it
pub fn get_remote_webc_checksum(url: &Url) -> Result<String, anyhow::Error> {
    let request_max_bytes = webc::WebC::get_signature_offset_start() + 4 + 1024 + 8 + 8;
    let data = get_webc_bytes(url, Some(0..request_max_bytes), None)
        .with_context(|| anyhow::anyhow!("note: use --registry to change the registry URL"))?
        .unwrap();
    let checksum = webc::WebC::get_checksum_bytes(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .to_vec();
    Ok(get_checksum_hash(&checksum))
}

/// Before fetching the entire file from a remote URL, just fetch the manifest
/// so we can see if the package has already been installed
pub fn get_remote_webc_manifest(url: &Url) -> Result<RemoteWebcInfo, anyhow::Error> {
    // Request up unti manifest size / manifest len
    let request_max_bytes = webc::WebC::get_signature_offset_start() + 4 + 1024 + 8 + 8;
    let data = get_webc_bytes(url, Some(0..request_max_bytes), None)?.unwrap();
    let checksum = webc::WebC::get_checksum_bytes(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_checksum_bytes failed")?
        .to_vec();
    let hex_string = get_checksum_hash(&checksum);

    let (manifest_start, manifest_len) = webc::WebC::get_manifest_offset_size(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_manifest_offset_size failed")?;
    let data_with_manifest =
        get_webc_bytes(url, Some(0..manifest_start + manifest_len), None)?.unwrap();
    let manifest = webc::WebC::get_manifest(&data_with_manifest)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_manifest failed")?;
    Ok(RemoteWebcInfo {
        checksum: hex_string,
        manifest,
    })
}

fn setup_client(
    url: &Url,
    application_type: &'static str,
) -> Result<reqwest::blocking::RequestBuilder, anyhow::Error> {
    let client = {
        let builder = reqwest::blocking::Client::builder();
        let builder = crate::graphql::proxy::maybe_set_up_proxy_blocking(builder)
            .context("setup_webc_client")?;
        builder
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("setup_webc_client: builder.build() failed")?
    };

    Ok(client.get(url.clone()).header(ACCEPT, application_type))
}

fn get_webc_bytes(
    url: &Url,
    range: Option<Range<usize>>,
    stream_response_into: Option<PathBuf>,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
    get_bytes(url, range, "application/webc", stream_response_into)
}

fn get_targz_bytes(
    url: &Url,
    range: Option<Range<usize>>,
    stream_response_into: Option<PathBuf>,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
    get_bytes(url, range, "application/tar+gzip", stream_response_into)
}

fn get_bytes(
    url: &Url,
    range: Option<Range<usize>>,
    application_type: &'static str,
    stream_response_into: Option<PathBuf>,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
    // curl -r 0-500 -L https://wapm.dev/syrusakbary/python -H "Accept: application/webc" --output python.webc

    let mut res = setup_client(url, application_type)?;

    if let Some(range) = range.as_ref() {
        res = res.header(RANGE, format!("bytes={}-{}", range.start, range.end));
    }

    let mut res = res
        .send()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("send() failed")?;

    if res.status().is_redirection() {
        return Err(anyhow::anyhow!("redirect: {:?}", res.status()));
    }

    if res.status().is_server_error() {
        return Err(anyhow::anyhow!("server error: {:?}", res.status()));
    }

    if res.status().is_client_error() {
        return Err(anyhow::anyhow!("client error: {:?}", res.status()));
    }

    if let Some(path) = stream_response_into.as_ref() {
        let mut file = std::fs::File::create(path).map_err(|e| {
            anyhow::anyhow!("failed to download {url} into {}: {e}", path.display())
        })?;

        res.copy_to(&mut file)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .map_err(|e| {
                anyhow::anyhow!("failed to download {url} into {}: {e}", path.display())
            })?;

        if application_type == "application/webc" {
            let mut buf = vec![0; 100];
            file.read_exact(&mut buf)
                .map_err(|e| anyhow::anyhow!("invalid webc downloaded from {url}: {e}"))?;
            if buf[0..webc::MAGIC.len()] != webc::MAGIC[..] {
                let first_100_bytes = String::from_utf8_lossy(&buf);
                return Err(anyhow::anyhow!("invalid webc bytes: {first_100_bytes:?}"));
            }
        }

        Ok(None)
    } else {
        let bytes = res
            .bytes()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("bytes() failed")?;

        if application_type == "application/webc"
            && (range.is_none() || range.unwrap().start == 0)
            && bytes[0..webc::MAGIC.len()] != webc::MAGIC[..]
        {
            let bytes = bytes.iter().copied().take(100).collect::<Vec<_>>();
            let first_100_bytes = String::from_utf8_lossy(&bytes);
            return Err(anyhow::anyhow!("invalid webc bytes: {first_100_bytes:?}"));
        }

        // else if "application/tar+gzip" - we would need to uncompress the response here
        // since failure responses are very small, this will fail during unpacking instead

        Ok(Some(bytes.to_vec()))
    }
}

// TODO: this test is segfaulting only on linux-musl, no other OS
// See https://github.com/wasmerio/wasmer/pull/3215
#[cfg(not(target_env = "musl"))]
#[test]
fn test_install_package() {
    println!("test install package...");
    let registry = "https://registry.wapm.io/graphql";
    if !test_if_registry_present(registry).unwrap_or(false) {
        panic!("registry.wapm.io not reachable, test will fail");
    }
    println!("registry present");

    let wabt = query_package_from_registry(registry, "wasmer/wabt", Some("1.0.29")).unwrap();

    println!("wabt queried: {wabt:#?}");

    assert_eq!(wabt.registry, registry);
    assert_eq!(wabt.package, "wasmer/wabt");
    assert_eq!(wabt.version, "1.0.29");
    assert_eq!(
        wabt.commands,
        "wat2wasm, wast2json, wasm2wat, wasm-interp, wasm-validate, wasm-strip"
    );
    assert_eq!(
        wabt.url,
        "https://registry-cdn.wapm.io/packages/wasmer/wabt/wabt-1.0.29.tar.gz".to_string()
    );

    let fake_wasmer_dir = tempdir::TempDir::new("tmp").unwrap();
    let wasmer_dir = fake_wasmer_dir.path();
    let path = install_package(wasmer_dir, &url::Url::parse(&wabt.url).unwrap()).unwrap();

    println!("package installed: {path:?}");

    assert_eq!(
        path,
        get_checkouts_dir(wasmer_dir).join(&format!("{}@1.0.29", Package::hash_url(&wabt.url)))
    );

    let all_installed_packages = get_all_local_packages(wasmer_dir);

    let is_installed = all_installed_packages
        .iter()
        .any(|p| p.name == "wasmer/wabt" && p.version == "1.0.29");

    if !is_installed {
        let panic_str = get_all_local_packages(wasmer_dir)
            .iter()
            .map(|p| format!("{} {} {}", p.registry, p.name, p.version))
            .collect::<Vec<_>>()
            .join("\r\n");
        panic!("get all local packages: failed to install:\r\n{panic_str}");
    }

    println!("ok, done");
}

/// A library that exposes bindings to a WAPM package.
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
