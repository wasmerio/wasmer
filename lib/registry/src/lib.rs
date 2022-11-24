//! High-level interactions with the WAPM backend.
//!
//! The GraphQL schema can be updated by running `make` in the Wasmer repo's
//! root directory.
//!
//! ```console
//! $ make update-graphql-schema
//! curl -sSfL https://registry.wapm.io/graphql/schema.graphql > lib/registry/graphql/schema.graphql
//! ```

use crate::config::Registries;
use anyhow::Context;
use core::ops::Range;
use reqwest::header::{ACCEPT, RANGE};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};
use url::Url;

pub mod config;
pub mod graphql;
pub mod login;
pub mod queries;
pub mod utils;

pub use crate::{
    config::{format_graphql, PartialWapmConfig},
    queries::get_bindings_query::ProgrammingLanguage,
};

pub static GLOBAL_CONFIG_FILE_NAME: &str = "wapm.toml";

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

pub fn get_package_local_dir(
    #[cfg(test)] test_name: &str,
    registry_host: &str,
    name: &str,
    version: &str,
) -> Result<PathBuf, String> {
    if !name.contains('/') {
        return Err(format!(
            "package name has to be in the format namespace/package: {name:?}"
        ));
    }
    let (namespace, name) = name
        .split_once('/')
        .ok_or_else(|| format!("missing namespace / name for {name:?}"))?;
    #[cfg(test)]
    let global_install_dir = get_global_install_dir(test_name, registry_host);
    #[cfg(not(test))]
    let global_install_dir = get_global_install_dir(registry_host);
    let install_dir = global_install_dir.ok_or_else(|| format!("no install dir for {name:?}"))?;
    Ok(install_dir.join(namespace).join(name).join(version))
}

pub fn try_finding_local_command(#[cfg(test)] test_name: &str, cmd: &str) -> Option<LocalPackage> {
    #[cfg(test)]
    let local_packages = get_all_local_packages(test_name, None);
    #[cfg(not(test))]
    let local_packages = get_all_local_packages(None);
    for p in local_packages {
        #[cfg(not(test))]
        let commands = p.get_commands();
        #[cfg(test)]
        let commands = p.get_commands(test_name);

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
}

impl LocalPackage {
    pub fn get_path(&self, #[cfg(test)] test_name: &str) -> Result<PathBuf, String> {
        let host = url::Url::parse(&self.registry)
            .ok()
            .and_then(|o| o.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| self.registry.clone());

        #[cfg(test)]
        {
            get_package_local_dir(test_name, &host, &self.name, &self.version)
        }

        #[cfg(not(test))]
        {
            get_package_local_dir(&host, &self.name, &self.version)
        }
    }
    pub fn get_commands(&self, #[cfg(test)] test_name: &str) -> Result<Vec<String>, String> {
        #[cfg(not(test))]
        let path = self.get_path()?;
        #[cfg(test)]
        let path = self.get_path(test_name)?;
        let toml_path = path.join("wapm.toml");
        let toml = std::fs::read_to_string(&toml_path)
            .map_err(|e| format!("error reading {}: {e}", toml_path.display()))?;
        let toml_parsed = toml::from_str::<wapm_toml::Manifest>(&toml)
            .map_err(|e| format!("error parsing {}: {e}", toml_path.display()))?;
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
    package_dir: &PathBuf,
    command: Option<&str>,
) -> Result<(wapm_toml::Manifest, PathBuf), anyhow::Error> {
    let wapm_toml = std::fs::read_to_string(package_dir.join("wapm.toml"))
        .map_err(|_| anyhow::anyhow!("Package {package_dir:?} has no wapm.toml"))?;

    let wapm_toml = toml::from_str::<wapm_toml::Manifest>(&wapm_toml)
        .map_err(|e| anyhow::anyhow!("Could not parse toml for {package_dir:?}: {e}"))?;

    let name = wapm_toml.package.name.clone();
    let version = wapm_toml.package.version.clone();

    let commands = wapm_toml.command.clone().unwrap_or_default();
    let entrypoint_module = match command {
        Some(s) => commands.iter().find(|c| c.get_name() == s).ok_or_else(|| {
            anyhow::anyhow!("Cannot run {name}@{version}: package has no command {s:?}")
        })?,
        None => commands.first().ok_or_else(|| {
            anyhow::anyhow!("Cannot run {name}@{version}: package has no commands")
        })?,
    };

    let module_name = entrypoint_module.get_module();
    let modules = wapm_toml.module.clone().unwrap_or_default();
    let entrypoint_module = modules
        .iter()
        .find(|m| m.name == module_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot run {name}@{version}: module {module_name} not found in wapm.toml"
            )
        })?;

    let entrypoint_source = package_dir.join(&entrypoint_module.source);

    Ok((wapm_toml, entrypoint_source))
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
pub fn get_all_local_packages(
    #[cfg(test)] test_name: &str,
    registry: Option<&str>,
) -> Vec<LocalPackage> {
    let mut packages = Vec::new();
    let registries = match registry {
        Some(s) => vec![s.to_string()],
        None => {
            #[cfg(test)]
            {
                get_all_available_registries(test_name).unwrap_or_default()
            }
            #[cfg(not(test))]
            {
                get_all_available_registries().unwrap_or_default()
            }
        }
    };

    let mut registry_hosts = registries
        .into_iter()
        .filter_map(|s| url::Url::parse(&s).ok()?.host_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();

    #[cfg(not(test))]
    let checkouts_dir = get_checkouts_dir();
    #[cfg(test)]
    let checkouts_dir = get_checkouts_dir(test_name);

    let mut registries_in_root_dir = checkouts_dir
        .as_ref()
        .map(get_all_names_in_dir)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(path, p)| if path.is_dir() { Some(p) } else { None })
        .collect();

    registry_hosts.append(&mut registries_in_root_dir);
    registry_hosts.sort();
    registry_hosts.dedup();

    for host in registry_hosts {
        #[cfg(not(test))]
        let global_install_dir = get_global_install_dir(&host);
        #[cfg(test)]
        let global_install_dir = get_global_install_dir(test_name, &host);
        let root_dir = match global_install_dir {
            Some(o) => o,
            None => continue,
        };

        for (username_path, user_name) in get_all_names_in_dir(&root_dir) {
            for (package_path, package_name) in get_all_names_in_dir(&username_path) {
                for (version_path, package_version) in get_all_names_in_dir(&package_path) {
                    let _ = match std::fs::read_to_string(version_path.join("wapm.toml")) {
                        Ok(o) => o,
                        Err(_) => continue,
                    };
                    packages.push(LocalPackage {
                        registry: host.clone(),
                        name: format!("{user_name}/{package_name}"),
                        version: package_version,
                    });
                }
            }
        }
    }

    packages
}

pub fn get_local_package(
    #[cfg(test)] test_name: &str,
    registry: Option<&str>,
    name: &str,
    version: Option<&str>,
) -> Option<LocalPackage> {
    #[cfg(not(test))]
    let local_packages = get_all_local_packages(registry);
    #[cfg(test)]
    let local_packages = get_all_local_packages(test_name, registry);

    local_packages
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
    use crate::{
        graphql::execute_query,
        queries::{get_package_by_command_query, GetPackageByCommandQuery},
    };
    use graphql_client::GraphQLQuery;

    let q = GetPackageByCommandQuery::build_query(get_package_by_command_query::Variables {
        command_name: command_name.to_string(),
    });

    let response: get_package_by_command_query::ResponseData = execute_query(registry_url, "", &q)
        .map_err(|e| format!("Error sending GetPackageByCommandQuery:  {e}"))?;

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
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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

#[test]
fn test_get_if_package_has_new_version() {
    const TEST_NAME: &str = "test_get_if_package_has_new_version";
    let fake_registry = "https://h0.com";
    let fake_name = "namespace0/project1";
    let fake_version = "1.0.0";

    let package_path = get_package_local_dir(TEST_NAME, "h0.com", fake_name, fake_version).unwrap();
    let _ = std::fs::remove_file(&package_path.join("wapm.toml"));
    let _ = std::fs::remove_file(&package_path.join("wapm.toml"));

    let r1 = get_if_package_has_new_version(
        TEST_NAME,
        fake_registry,
        "namespace0/project1",
        Some(fake_version.to_string()),
        Duration::from_secs(5 * 60),
    );

    assert_eq!(
        r1.unwrap(),
        GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
            registry_url: fake_registry.to_string(),
            namespace: "namespace0".to_string(),
            name: "project1".to_string(),
            version: Some(fake_version.to_string()),
        }
    );

    let package_path = get_package_local_dir(TEST_NAME, "h0.com", fake_name, fake_version).unwrap();
    std::fs::create_dir_all(&package_path).unwrap();
    std::fs::write(&package_path.join("wapm.toml"), b"").unwrap();

    let r1 = get_if_package_has_new_version(
        TEST_NAME,
        fake_registry,
        "namespace0/project1",
        Some(fake_version.to_string()),
        Duration::from_secs(5 * 60),
    );

    assert_eq!(
        r1.unwrap(),
        GetIfPackageHasNewVersionResult::UseLocalAlreadyInstalled {
            registry_host: "h0.com".to_string(),
            namespace: "namespace0".to_string(),
            name: "project1".to_string(),
            version: fake_version.to_string(),
            path: package_path,
        }
    );
}

/// Returns true if a package has a newer version
///
/// Also returns true if the package is not installed yet.
pub fn get_if_package_has_new_version(
    #[cfg(test)] test_name: &str,
    registry_url: &str,
    name: &str,
    version: Option<String>,
    max_timeout: Duration,
) -> Result<GetIfPackageHasNewVersionResult, String> {
    let host = match url::Url::parse(registry_url) {
        Ok(o) => match o.host_str().map(|s| s.to_string()) {
            Some(s) => s,
            None => return Err(format!("invalid host: {registry_url}")),
        },
        Err(_) => return Err(format!("invalid host: {registry_url}")),
    };

    let (namespace, name) = name
        .split_once('/')
        .ok_or_else(|| format!("missing namespace / name for {name:?}"))?;

    #[cfg(not(test))]
    let global_install_dir = get_global_install_dir(&host);
    #[cfg(test)]
    let global_install_dir = get_global_install_dir(test_name, &host);

    let package_dir = global_install_dir.map(|path| path.join(namespace).join(name));

    let package_dir = match package_dir {
        Some(s) => s,
        None => {
            return Ok(GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
                registry_url: registry_url.to_string(),
                namespace: namespace.to_string(),
                name: name.to_string(),
                version,
            })
        }
    };

    // if version is specified: look if that specific version exists
    if let Some(s) = version.as_ref() {
        let installed_path = package_dir.join(s).join("wapm.toml");
        if installed_path.exists() {
            return Ok(GetIfPackageHasNewVersionResult::UseLocalAlreadyInstalled {
                registry_host: host,
                namespace: namespace.to_string(),
                name: name.to_string(),
                version: s.clone(),
                path: package_dir.join(s),
            });
        } else {
            return Ok(GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
                registry_url: registry_url.to_string(),
                namespace: namespace.to_string(),
                name: name.to_string(),
                version: Some(s.clone()),
            });
        }
    }

    // version has not been explicitly specified: check if any package < duration exists
    let read_dir = match std::fs::read_dir(&package_dir) {
        Ok(o) => o,
        Err(_) => {
            return Ok(GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
                registry_url: registry_url.to_string(),
                namespace: namespace.to_string(),
                name: name.to_string(),
                version,
            });
        }
    };

    // all installed versions of this package
    let all_installed_versions = read_dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let version = semver::Version::parse(entry.file_name().to_str()?).ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            let older_than_timeout = modified.elapsed().ok()? > max_timeout;
            Some((version, older_than_timeout))
        })
        .collect::<Vec<_>>();

    if all_installed_versions.is_empty() {
        // package not installed yet
        Ok(GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
            registry_url: registry_url.to_string(),
            namespace: namespace.to_string(),
            name: name.to_string(),
            version,
        })
    } else if all_installed_versions
        .iter()
        .all(|(_, older_than_timeout)| *older_than_timeout)
    {
        // all packages are older than the timeout: there might be a new package available
        return Ok(GetIfPackageHasNewVersionResult::LocalVersionMayBeOutdated {
            registry_host: registry_url.to_string(),
            namespace: namespace.to_string(),
            name: name.to_string(),
            installed_versions: all_installed_versions
                .iter()
                .map(|(key, old)| (format!("{key}"), *old))
                .collect::<Vec<_>>(),
        });
    } else {
        // return the package that was younger than timeout
        let younger_than_timeout_version = all_installed_versions
            .iter()
            .find(|(_, older_than_timeout)| !older_than_timeout)
            .unwrap();
        let version = format!("{}", younger_than_timeout_version.0);
        let installed_path = package_dir.join(&version).join("wapm.toml");
        if installed_path.exists() {
            Ok(GetIfPackageHasNewVersionResult::UseLocalAlreadyInstalled {
                registry_host: host,
                namespace: namespace.to_string(),
                name: name.to_string(),
                version: version.clone(),
                path: package_dir.join(&version),
            })
        } else {
            Ok(GetIfPackageHasNewVersionResult::PackageNotInstalledYet {
                registry_url: registry_url.to_string(),
                namespace: namespace.to_string(),
                name: name.to_string(),
                version: None,
            })
        }
    }
}

/// Returns the download info of the packages, on error returns all the available packages
/// i.e. (("foo/python", "wapm.io"), ("bar/python" "wapm.io")))
pub fn query_package_from_registry(
    registry_url: &str,
    name: &str,
    version: Option<&str>,
) -> Result<PackageDownloadInfo, QueryPackageError> {
    use crate::{
        graphql::execute_query,
        queries::{get_package_version_query, GetPackageVersionQuery},
    };
    use graphql_client::GraphQLQuery;

    let q = GetPackageVersionQuery::build_query(get_package_version_query::Variables {
        name: name.to_string(),
        version: version.map(|s| s.to_string()),
    });

    let response: get_package_version_query::ResponseData = execute_query(registry_url, "", &q)
        .map_err(|e| {
            QueryPackageError::ErrorSendingQuery(format!("Error sending GetPackagesQuery: {e}"))
        })?;

    let v = response.package_version.as_ref().ok_or_else(|| {
        QueryPackageError::ErrorSendingQuery(format!(
            "Invalid response for crate {name:?}: no package version: {response:#?}"
        ))
    })?;

    let manifest = toml::from_str::<wapm_toml::Manifest>(&v.manifest).map_err(|e| {
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

pub fn get_wasmer_root_dir(#[cfg(test)] test_name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    {
        PartialWapmConfig::get_folder(test_name).ok()
    }
    #[cfg(not(test))]
    {
        PartialWapmConfig::get_folder().ok()
    }
}

pub fn get_checkouts_dir(#[cfg(test)] test_name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    let root_dir = get_wasmer_root_dir(test_name)?;
    #[cfg(not(test))]
    let root_dir = get_wasmer_root_dir()?;
    Some(root_dir.join("checkouts"))
}

pub fn get_webc_dir(#[cfg(test)] test_name: &str) -> Option<PathBuf> {
    #[cfg(test)]
    let root_dir = get_wasmer_root_dir(test_name)?;
    #[cfg(not(test))]
    let root_dir = get_wasmer_root_dir()?;
    Some(root_dir.join("webc"))
}

/// Returs the path to the directory where all packages on this computer are being stored
pub fn get_global_install_dir(
    #[cfg(test)] test_name: &str,
    registry_host: &str,
) -> Option<PathBuf> {
    #[cfg(test)]
    let root_dir = get_checkouts_dir(test_name)?;
    #[cfg(not(test))]
    let root_dir = get_checkouts_dir()?;
    Some(root_dir.join(registry_host))
}

/// Convenience function that will unpack .tar.gz files and .tar.bz
/// files to a target directory (does NOT remove the original .tar.gz)
pub fn try_unpack_targz<P: AsRef<Path>>(
    target_targz_path: P,
    target_path: P,
    strip_toplevel: bool,
) -> Result<PathBuf, anyhow::Error> {
    let target_targz_path = target_targz_path.as_ref();
    let target_path = target_path.as_ref();
    let open_file = || {
        std::fs::File::open(&target_targz_path)
            .map_err(|e| anyhow::anyhow!("failed to open {}: {e}", target_targz_path.display()))
    };

    let try_decode_gz = || {
        let file = open_file()?;
        let gz_decoded = flate2::read::GzDecoder::new(&file);
        let mut ar = tar::Archive::new(gz_decoded);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!("failed to unpack {}: {e}", target_targz_path.display())
            })
        } else {
            ar.unpack(target_path).map_err(|e| {
                anyhow::anyhow!("failed to unpack {}: {e}", target_targz_path.display())
            })
        }
    };

    let try_decode_xz = || {
        let file = open_file()?;
        let mut decomp: Vec<u8> = Vec::new();
        let mut bufread = std::io::BufReader::new(&file);
        lzma_rs::xz_decompress(&mut bufread, &mut decomp).map_err(|e| {
            anyhow::anyhow!("failed to unpack {}: {e}", target_targz_path.display())
        })?;

        let cursor = std::io::Cursor::new(decomp);
        let mut ar = tar::Archive::new(cursor);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!("failed to unpack {}: {e}", target_targz_path.display())
            })
        } else {
            ar.unpack(target_path).map_err(|e| {
                anyhow::anyhow!("failed to unpack {}: {e}", target_targz_path.display())
            })
        }
    };

    try_decode_gz().or_else(|_| try_decode_xz())?;

    Ok(target_targz_path.to_path_buf())
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

pub fn unpack_sans_parent<R>(mut archive: tar::Archive<R>, dst: &Path) -> std::io::Result<()>
where
    R: std::io::Read,
{
    use std::path::Component::Normal;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path: PathBuf = entry
            .path()?
            .components()
            .skip(1) // strip top-level directory
            .filter(|c| matches!(c, Normal(_))) // prevent traversal attacks
            .collect();
        entry.unpack(dst.join(path))?;
    }
    Ok(())
}

/// Given a triple of [registry, name, version], downloads and installs the
/// .tar.gz if it doesn't yet exist, returns the (package dir, entrypoint .wasm file path)
pub fn install_package(
    #[cfg(test)] test_name: &str,
    registry: Option<&str>,
    name: &str,
    version: Option<&str>,
    package_download_info: Option<PackageDownloadInfo>,
    force_install: bool,
) -> Result<(LocalPackage, PathBuf), String> {
    let package_info = match package_download_info {
        Some(s) => s,
        None => {
            let registries = match registry {
                Some(s) => vec![s.to_string()],
                None => {
                    #[cfg(test)]
                    {
                        get_all_available_registries(test_name)?
                    }
                    #[cfg(not(test))]
                    {
                        get_all_available_registries()?
                    }
                }
            };
            let mut url_of_package = None;

            let version_str = match version {
                None => name.to_string(),
                Some(v) => format!("{name}@{v}"),
            };

            let registries_searched = registries
                .iter()
                .filter_map(|s| url::Url::parse(s).ok())
                .filter_map(|s| Some(s.host_str()?.to_string()))
                .collect::<Vec<_>>();

            let mut errors = BTreeMap::new();

            for r in registries.iter() {
                if !force_install {
                    #[cfg(not(test))]
                    let package_has_new_version = get_if_package_has_new_version(
                        r,
                        name,
                        version.map(|s| s.to_string()),
                        Duration::from_secs(60 * 5),
                    )?;
                    #[cfg(test)]
                    let package_has_new_version = get_if_package_has_new_version(
                        test_name,
                        r,
                        name,
                        version.map(|s| s.to_string()),
                        Duration::from_secs(60 * 5),
                    )?;
                    if let GetIfPackageHasNewVersionResult::UseLocalAlreadyInstalled {
                        registry_host,
                        namespace,
                        name,
                        version,
                        path,
                    } = package_has_new_version
                    {
                        return Ok((
                            LocalPackage {
                                registry: registry_host,
                                name: format!("{namespace}/{name}"),
                                version,
                            },
                            path,
                        ));
                    }
                }

                match query_package_from_registry(r, name, version) {
                    Ok(o) => {
                        url_of_package = Some((r, o));
                        break;
                    }
                    Err(e) => {
                        errors.insert(r.clone(), e);
                    }
                }
            }

            let errors = errors
                .into_iter()
                .map(|(registry, e)| format!("  {registry}: {e}"))
                .collect::<Vec<_>>()
                .join("\r\n");

            let (_, package_info) = url_of_package.ok_or_else(|| {
                format!("Package {version_str} not found in registries {registries_searched:?}.\r\n\r\nErrors:\r\n\r\n{errors}")
            })?;

            package_info
        }
    };

    let host = url::Url::parse(&package_info.registry)
        .map_err(|e| format!("invalid url: {}: {e}", package_info.registry))?
        .host_str()
        .ok_or_else(|| format!("invalid url: {}", package_info.registry))?
        .to_string();

    #[cfg(test)]
    let dir = get_package_local_dir(
        test_name,
        &host,
        &package_info.package,
        &package_info.version,
    )?;
    #[cfg(not(test))]
    let dir = get_package_local_dir(&host, &package_info.package, &package_info.version)?;

    let version = package_info.version;
    let name = package_info.package;

    if !dir.join("wapm.toml").exists() || force_install {
        download_and_unpack_targz(&package_info.url, &dir, false).map_err(|e| format!("{e}"))?;
    }

    Ok((
        LocalPackage {
            registry: package_info.registry,
            name,
            version,
        },
        dir,
    ))
}

pub fn whoami(
    #[cfg(test)] test_name: &str,
    registry: Option<&str>,
) -> Result<(String, String), anyhow::Error> {
    use crate::queries::{who_am_i_query, WhoAmIQuery};
    use graphql_client::GraphQLQuery;

    #[cfg(test)]
    let config = PartialWapmConfig::from_file(test_name);
    #[cfg(not(test))]
    let config = PartialWapmConfig::from_file();

    let config = config
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("{registry:?}"))?;

    let registry = match registry {
        Some(s) => format_graphql(s),
        None => config.registry.get_current_registry(),
    };

    let login_token = config
        .registry
        .get_login_token_for_registry(&registry)
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
    use crate::queries::{test_if_registry_present, TestIfRegistryPresent};
    use graphql_client::GraphQLQuery;

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

pub fn get_all_available_registries(#[cfg(test)] test_name: &str) -> Result<Vec<String>, String> {
    #[cfg(test)]
    let config = PartialWapmConfig::from_file(test_name)?;
    #[cfg(not(test))]
    let config = PartialWapmConfig::from_file()?;

    let mut registries = Vec::new();
    match config.registry {
        Registries::Single(s) => {
            registries.push(format_graphql(&s.url));
        }
        Registries::Multi(m) => {
            for key in m.tokens.keys() {
                registries.push(format_graphql(key));
            }
        }
    }
    Ok(registries)
}

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteWebcInfo {
    pub checksum: String,
    pub manifest: webc::Manifest,
}

pub fn install_webc_package(
    #[cfg(test)] test_name: &str,
    url: &Url,
    checksum: &str,
) -> Result<(), anyhow::Error> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            {
                #[cfg(test)]
                {
                    install_webc_package_inner(test_name, url, checksum).await
                }
                #[cfg(not(test))]
                {
                    install_webc_package_inner(url, checksum).await
                }
            }
        })
}

async fn install_webc_package_inner(
    #[cfg(test)] test_name: &str,
    url: &Url,
    checksum: &str,
) -> Result<(), anyhow::Error> {
    use futures_util::StreamExt;

    #[cfg(test)]
    let path = get_webc_dir(test_name).ok_or_else(|| anyhow::anyhow!("no webc dir"))?;
    #[cfg(not(test))]
    let path = get_webc_dir().ok_or_else(|| anyhow::anyhow!("no webc dir"))?;

    let _ = std::fs::create_dir_all(&path);

    let webc_path = path.join(checksum);

    let mut file = std::fs::File::create(&webc_path)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context(anyhow::anyhow!("{}", webc_path.display()))?;

    let client = {
        let builder = reqwest::Client::builder();
        let builder = crate::graphql::proxy::maybe_set_up_proxy(builder)?;
        builder
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
#[cfg(test)]
pub fn get_all_installed_webc_packages(test_name: &str) -> Vec<RemoteWebcInfo> {
    get_all_installed_webc_packages_inner(test_name)
}

#[cfg(not(test))]
pub fn get_all_installed_webc_packages() -> Vec<RemoteWebcInfo> {
    get_all_installed_webc_packages_inner("")
}

fn get_all_installed_webc_packages_inner(_test_name: &str) -> Vec<RemoteWebcInfo> {
    #[cfg(test)]
    let dir = match get_webc_dir(_test_name) {
        Some(s) => s,
        None => return Vec::new(),
    };

    #[cfg(not(test))]
    let dir = match get_webc_dir() {
        Some(s) => s,
        None => return Vec::new(),
    };

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
    hex::encode(&checksum)
}

/// Returns the checksum of the .webc file, so that we can check whether the
/// file is already installed before downloading it
pub fn get_remote_webc_checksum(url: &Url) -> Result<String, anyhow::Error> {
    let request_max_bytes = webc::WebC::get_signature_offset_start() + 4 + 1024 + 8 + 8;
    let data = get_webc_bytes(url, Some(0..request_max_bytes))
        .context("get_webc_bytes failed on {url}")?;
    let checksum = webc::WebC::get_checksum_bytes(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("get_checksum_bytes failed")?
        .to_vec();
    Ok(get_checksum_hash(&checksum))
}

/// Before fetching the entire file from a remote URL, just fetch the manifest
/// so we can see if the package has already been installed
pub fn get_remote_webc_manifest(url: &Url) -> Result<RemoteWebcInfo, anyhow::Error> {
    // Request up unti manifest size / manifest len
    let request_max_bytes = webc::WebC::get_signature_offset_start() + 4 + 1024 + 8 + 8;
    let data = get_webc_bytes(url, Some(0..request_max_bytes))?;
    let checksum = webc::WebC::get_checksum_bytes(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_checksum_bytes failed")?
        .to_vec();
    let hex_string = get_checksum_hash(&checksum);

    let (manifest_start, manifest_len) = webc::WebC::get_manifest_offset_size(&data)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_manifest_offset_size failed")?;
    let data_with_manifest = get_webc_bytes(url, Some(0..manifest_start + manifest_len))?;
    let manifest = webc::WebC::get_manifest(&data_with_manifest)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("WebC::get_manifest failed")?;
    Ok(RemoteWebcInfo {
        checksum: hex_string,
        manifest,
    })
}

fn setup_webc_client(url: &Url) -> Result<reqwest::blocking::RequestBuilder, anyhow::Error> {
    let client = {
        let builder = reqwest::blocking::Client::builder();
        let builder = crate::graphql::proxy::maybe_set_up_proxy_blocking(builder)
            .context("setup_webc_client")?;
        builder
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("setup_webc_client: builder.build() failed")?
    };

    Ok(client.get(url.clone()).header(ACCEPT, "application/webc"))
}

fn get_webc_bytes(url: &Url, range: Option<Range<usize>>) -> Result<Vec<u8>, anyhow::Error> {
    // curl -r 0-500 -L https://wapm.dev/syrusakbary/python -H "Accept: application/webc" --output python.webc

    let mut res = setup_webc_client(url)?;

    if let Some(range) = range.as_ref() {
        res = res.header(RANGE, format!("bytes={}-{}", range.start, range.end));
    }

    let res = res
        .send()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("send() failed")?;
    let bytes = res
        .bytes()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("bytes() failed")?;

    Ok(bytes.to_vec())
}

// TODO: this test is segfaulting only on linux-musl, no other OS
// See https://github.com/wasmerio/wasmer/pull/3215
#[cfg(not(target_env = "musl"))]
#[test]
fn test_install_package() {
    const TEST_NAME: &str = "test_install_package";

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

    let (package, _) = install_package(
        TEST_NAME,
        Some(registry),
        "wasmer/wabt",
        Some("1.0.29"),
        None,
        true,
    )
    .unwrap();

    println!("package installed: {package:#?}");

    assert_eq!(
        package.get_path(TEST_NAME).unwrap(),
        get_global_install_dir(TEST_NAME, "registry.wapm.io")
            .unwrap()
            .join("wasmer")
            .join("wabt")
            .join("1.0.29")
    );

    let all_installed_packages = get_all_local_packages(TEST_NAME, Some(registry));

    println!("all_installed_packages: {all_installed_packages:#?}");

    let is_installed = all_installed_packages
        .iter()
        .any(|p| p.name == "wasmer/wabt" && p.version == "1.0.29");

    println!("is_installed: {is_installed:#?}");

    if !is_installed {
        let panic_str = get_all_local_packages(TEST_NAME, Some(registry))
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

impl Display for BindingsGenerator {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
    use crate::queries::{
        get_bindings_query::{ResponseData, Variables},
        GetBindingsQuery,
    };
    use graphql_client::GraphQLQuery;

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
