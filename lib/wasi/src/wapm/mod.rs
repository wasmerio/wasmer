use anyhow::{bail, Context};
use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};
use virtual_fs::FileSystem;

use tracing::*;
#[allow(unused_imports)]
use tracing::{error, warn};
use webc::{
    metadata::{Annotation, UrlOrManifest},
    v1::WebC,
};

use crate::{
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    WasiRuntime,
};

mod pirita;

use crate::http::{DynHttpClient, HttpRequest, HttpRequestOptions};
use pirita::*;

pub(crate) fn fetch_webc_task(
    cache_dir: &Path,
    webc: &str,
    runtime: &dyn WasiRuntime,
) -> Result<BinaryPackage, anyhow::Error> {
    let client = runtime
        .http_client()
        .context("no http client available")?
        .clone();

    let f = async move { fetch_webc(cache_dir, webc, client).await };

    let result = runtime
        .task_manager()
        .block_on(f)
        .context("webc fetch task has died");
    result.with_context(|| format!("could not fetch webc '{webc}'"))
}

async fn fetch_webc(
    cache_dir: &Path,
    webc: &str,
    client: DynHttpClient,
) -> Result<BinaryPackage, anyhow::Error> {
    let name = webc.split_once(':').map(|a| a.0).unwrap_or_else(|| webc);
    let (name, version) = match name.split_once('@') {
        Some((name, version)) => (name, Some(version)),
        None => (name, None),
    };
    let url_query = match version {
        Some(version) => WAPM_WEBC_QUERY_SPECIFIC
            .replace(WAPM_WEBC_QUERY_TAG, name.replace('\"', "'").as_str())
            .replace(WAPM_WEBC_VERSION_TAG, version.replace('\"', "'").as_str()),
        None => WAPM_WEBC_QUERY_LAST.replace(WAPM_WEBC_QUERY_TAG, name.replace('\"', "'").as_str()),
    };
    debug!("request: {}", url_query);

    let url = format!(
        "{}{}",
        WAPM_WEBC_URL,
        urlencoding::encode(url_query.as_str())
    );

    let response = client
        .request(HttpRequest {
            url,
            method: "GET".to_string(),
            headers: vec![],
            body: None,
            options: HttpRequestOptions::default(),
        })
        .await?;

    if response.status != 200 {
        bail!(" http request failed with status {}", response.status);
    }
    let body = response.body.context("HTTP response with empty body")?;
    let data: WapmWebQuery =
        serde_json::from_slice(&body).context("Could not parse webc registry JSON data")?;
    debug!("response: {:?}", data);

    let PiritaVersionedDownload {
        url: download_url,
        version,
    } = wapm_extract_version(&data).context("No pirita download URL available")?;
    let mut pkg = download_webc(cache_dir, name, download_url, client).await?;
    pkg.version = version.into();
    Ok(pkg)
}

struct PiritaVersionedDownload {
    url: String,
    version: String,
}

fn wapm_extract_version(data: &WapmWebQuery) -> Option<PiritaVersionedDownload> {
    if let Some(package) = &data.data.get_package_version {
        let url = package.distribution.pirita_download_url.clone()?;
        Some(PiritaVersionedDownload {
            url,
            version: package.version.clone(),
        })
    } else if let Some(package) = &data.data.get_package {
        let url = package
            .last_version
            .distribution
            .pirita_download_url
            .clone()?;
        Some(PiritaVersionedDownload {
            url,
            version: package.last_version.version.clone(),
        })
    } else {
        None
    }
}

pub fn parse_static_webc(data: Vec<u8>) -> Result<BinaryPackage, anyhow::Error> {
    let options = webc::v1::ParseOptions::default();
    match webc::v1::WebCOwned::parse(data, &options) {
        Ok(webc) => unsafe {
            let webc = Arc::new(webc);
            return parse_webc(webc.as_webc_ref(), webc.clone())
                .with_context(|| "Could not parse webc".to_string());
        },
        Err(err) => {
            warn!("failed to parse WebC: {}", err);
            Err(err.into())
        }
    }
}

async fn download_webc(
    cache_dir: &Path,
    name: &str,
    pirita_download_url: String,
    client: DynHttpClient,
) -> Result<BinaryPackage, anyhow::Error> {
    let mut name_comps = pirita_download_url
        .split('/')
        .collect::<Vec<_>>()
        .into_iter()
        .rev();
    let mut name = name_comps.next().unwrap_or(name);
    let mut name_store;
    for _ in 0..2 {
        if let Some(prefix) = name_comps.next() {
            name_store = format!("{}_{}", prefix, name);
            name = name_store.as_str();
        }
    }
    let compute_path = |cache_dir: &Path, name: &str| {
        let name = name.replace('/', "._.");
        std::path::Path::new(cache_dir).join(&name)
    };

    // build the parse options
    let options = webc::v1::ParseOptions::default();

    // fast path
    let path = compute_path(cache_dir, name);

    #[cfg(feature = "sys")]
    if path.exists() {
        match webc::v1::WebCMmap::parse(path.clone(), &options) {
            Ok(webc) => unsafe {
                let webc = Arc::new(webc);
                return parse_webc(webc.as_webc_ref(), webc.clone()).with_context(|| {
                    format!("could not parse webc file at path : '{}'", path.display())
                });
            },
            Err(err) => {
                warn!("failed to parse WebC: {}", err);
            }
        }
    }
    if let Ok(data) = std::fs::read(&path) {
        if let Ok(webc) = parse_static_webc(data) {
            return Ok(webc);
        }
    }

    // slow path
    let data = download_package(&pirita_download_url, client)
        .await
        .with_context(|| {
            format!(
                "Could not download webc package from '{}'",
                pirita_download_url
            )
        })?;

    #[cfg(feature = "sys")]
    {
        let path = compute_path(cache_dir, name);
        std::fs::create_dir_all(path.parent().unwrap()).with_context(|| {
            format!("Could not create cache directory '{}'", cache_dir.display())
        })?;

        let mut temp_path = path.clone();
        let rand_128: u128 = rand::random();
        temp_path = std::path::PathBuf::from(format!(
            "{}.{}.temp",
            temp_path.as_os_str().to_string_lossy(),
            rand_128
        ));

        if let Err(err) = std::fs::write(temp_path.as_path(), &data[..]) {
            debug!(
                "failed to write webc cache file [{}] - {}",
                temp_path.as_path().to_string_lossy(),
                err
            );
        }
        if let Err(err) = std::fs::rename(temp_path.as_path(), path.as_path()) {
            debug!(
                "failed to rename webc cache file [{}] - {}",
                temp_path.as_path().to_string_lossy(),
                err
            );
        }

        match webc::v1::WebCMmap::parse(path.clone(), &options) {
            Ok(webc) => unsafe {
                let webc = Arc::new(webc);
                return parse_webc(webc.as_webc_ref(), webc.clone())
                    .with_context(|| format!("Could not parse webc at path '{}'", path.display()));
            },
            Err(err) => {
                warn!("failed to parse WebC: {}", err);
            }
        }
    }

    let webc_raw = webc::v1::WebCOwned::parse(data, &options)
        .with_context(|| format!("Failed to parse downloaded from '{pirita_download_url}'"))?;
    let webc = Arc::new(webc_raw);
    // FIXME: add SAFETY comment
    let package = unsafe {
        parse_webc(webc.as_webc_ref(), webc.clone()).context("Could not parse binary package")?
    };

    Ok(package)
}

async fn download_package(
    download_url: &str,
    client: DynHttpClient,
) -> Result<Vec<u8>, anyhow::Error> {
    let request = HttpRequest {
        url: download_url.to_string(),
        method: "GET".to_string(),
        headers: vec![],
        body: None,
        options: HttpRequestOptions {
            gzip: true,
            cors_proxy: None,
        },
    };
    let response = client.request(request).await?;
    if response.status != 200 {
        bail!("HTTP request failed with status {}", response.status);
    }
    response.body.context("HTTP response with empty body")
}

// TODO: should return Result<_, anyhow::Error>
unsafe fn parse_webc<'a, T>(webc: webc::v1::WebC<'a>, ownership: Arc<T>) -> Option<BinaryPackage>
where
    T: std::fmt::Debug + Send + Sync + 'static,
    T: Deref<Target = WebC<'static>>,
{
    let package_name = webc.get_package_name();

    let mut pck = webc
        .manifest
        .entrypoint
        .iter()
        .filter_map(|entry| webc.manifest.commands.get(entry).map(|a| (a, entry)))
        .filter_map(|(cmd, entry)| {
            let api = if cmd.runner.starts_with("https://webc.org/runner/emscripten") {
                "emscripten"
            } else if cmd.runner.starts_with("https://webc.org/runner/wasi") {
                "wasi"
            } else {
                warn!("unsupported runner - {}", cmd.runner);
                return None;
            };
            let atom = webc.get_atom_name_for_command(api, entry.as_str());
            match atom {
                Ok(a) => Some(a),
                Err(err) => {
                    warn!(
                        "failed to find atom name for entry command({}) - {} - falling back on the command name itself",
                        entry.as_str(),
                        err
                    );
                    for (name, atom) in webc.manifest.atoms.iter() {
                        tracing::debug!("found atom (name={}, kind={})", name, atom.kind);
                    }
                    Some(entry.clone())
                }
            }
        })
        .filter_map(|atom| match webc.get_atom(&package_name, atom.as_str()) {
            Ok(a) => Some(a),
            Err(err) => {
                warn!("failed to find atom for atom name({}) - {}", atom, err);
                None
            }
        })
        .map(|atom| {
            BinaryPackage::new_with_ownership(
                package_name.as_str(),
                Some(atom.into()),
                ownership.clone(),
            )
        })
        .next();

    // Otherwise add a package without an entry point
    if pck.is_none() {
        pck = Some(BinaryPackage::new_with_ownership(
            package_name.as_str(),
            None,
            ownership.clone(),
        ))
    }
    let mut pck = pck.take().unwrap();

    // Add all the dependencies
    for uses in webc.manifest.use_map.values() {
        let uses = match uses {
            UrlOrManifest::Url(url) => Some(url.path().to_string()),
            UrlOrManifest::Manifest(manifest) => manifest.origin.clone(),
            UrlOrManifest::RegistryDependentUrl(url) => Some(url.clone()),
        };
        if let Some(uses) = uses {
            pck.uses.push(uses);
        }
    }

    // Set the version of this package
    if let Some(Annotation::Map(wapm)) = webc.manifest.package.get("wapm") {
        if let Some(Annotation::Text(version)) = wapm.get(&Annotation::Text("version".to_string()))
        {
            pck.version = version.clone().into();
        }
    } else if let Some(Annotation::Text(version)) = webc.manifest.package.get("version") {
        pck.version = version.clone().into();
    }

    // Add the file system from the webc
    let webc_fs = virtual_fs::webc_fs::WebcFileSystem::init_all(ownership.clone());
    let top_level_dirs = webc_fs.top_level_dirs().clone();
    pck.webc_fs = Some(Arc::new(webc_fs));
    pck.webc_top_level_dirs = top_level_dirs;

    // Add the memory footprint of the file system
    if let Some(webc_fs) = pck.webc_fs.as_ref() {
        let root_path = PathBuf::from("/");
        pck.file_system_memory_footprint +=
            count_file_system(webc_fs.as_ref(), root_path.as_path());
    }

    // Add all the commands
    for (command, action) in webc.get_metadata().commands.iter() {
        let api = if action
            .runner
            .starts_with("https://webc.org/runner/emscripten")
        {
            "emscripten"
        } else if action.runner.starts_with("https://webc.org/runner/wasi") {
            "wasi"
        } else {
            warn!("unsupported runner - {}", action.runner);
            continue;
        };
        let atom = webc.get_atom_name_for_command(api, command.as_str());
        let atom = match atom {
            Ok(a) => Some(a),
            Err(err) => {
                debug!(
                    "failed to find atom name for entry command({}) - {} - falling back on the command name itself",
                    command.as_str(),
                    err
                );
                Some(command.clone())
            }
        };

        // Load the atom as a command
        if let Some(atom_name) = atom {
            match webc.get_atom(package_name.as_str(), atom_name.as_str()) {
                Ok(atom) => {
                    trace!(
                        "added atom (name={}, size={}) for command [{}]",
                        atom_name,
                        atom.len(),
                        command
                    );
                    if pck.entry.is_none() {
                        trace!("defaulting entry to command [{}]", command);
                        let entry: &'static [u8] = {
                            let atom: &'_ [u8] = atom;
                            std::mem::transmute(atom)
                        };
                        pck.entry = Some(entry.into());
                    }

                    let mut commands = pck.commands.write().unwrap();
                    commands.push(BinaryPackageCommand::new_with_ownership(
                        command.clone(),
                        atom.into(),
                        ownership.clone(),
                    ));
                }
                Err(err) => {
                    debug!(
                        "Failed to find atom [{}].[{}] - {} - falling back on the first atom",
                        package_name, atom_name, err
                    );

                    if let Ok(files) = webc.atoms.get_all_files_and_directories_with_bytes() {
                        if let Some(file) = files.iter().next() {
                            if let Some(atom) = file.get_bytes() {
                                trace!(
                                    "added atom (name={}, size={}) for command [{}]",
                                    atom_name,
                                    atom.len(),
                                    command
                                );
                                let mut commands = pck.commands.write().unwrap();
                                commands.push(BinaryPackageCommand::new_with_ownership(
                                    command.clone(),
                                    atom.into(),
                                    ownership.clone(),
                                ));
                                continue;
                            }
                        }
                    }

                    debug!(
                        "Failed to find atom [{}].[{}] - {} - command will be ignored",
                        package_name, package_name, err
                    );
                    for (name, atom) in webc.manifest.atoms.iter() {
                        tracing::debug!("found atom (name={}, kind={})", name, atom.kind);
                    }
                    if let Ok(files) = webc.atoms.get_all_files_and_directories_with_bytes() {
                        for file in files.iter() {
                            tracing::debug!("found file ({})", file.get_path().to_string_lossy());
                        }
                    }
                }
            }
        }
    }

    Some(pck)
}

fn count_file_system(fs: &dyn FileSystem, path: &Path) -> u64 {
    let mut total = 0;

    let dir = match fs.read_dir(path) {
        Ok(d) => d,
        Err(_err) => {
            // TODO: propagate error?
            return 0;
        }
    };

    for res in dir {
        match res {
            Ok(entry) => {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                    if meta.is_dir() {
                        total += count_file_system(fs, entry.path.as_path());
                    }
                }
            }
            Err(_err) => {
                // TODO: propagate error?
            }
        };
    }

    total
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::{BTreeMap, HashMap},
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");
    const COREUTILS: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/coreutils-1.0.14-076508e5-e704-463f-b467-f3d9658fc907.webc");
    const BASH: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/bash-1.0.12-0103d733-1afb-4a56-b0ef-0e124139e996.webc");

    #[test]
    fn parse_the_python_webc_file() {
        let python = webc::compat::Container::from_bytes(PYTHON).unwrap();

        let pkg = parse_static_webc(PYTHON.to_vec()).unwrap();

        assert_eq!(pkg.package_name, "python");
        assert_eq!(pkg.version, "0.1.0");
        assert_eq!(pkg.uses, Vec::<String>::new());
        assert_eq!(pkg.envs, HashMap::new());
        assert_eq!(pkg.base_dir, None);
        assert_eq!(pkg.mappings, Vec::<String>::new());
        assert_eq!(pkg.module_memory_footprint, 4694941);
        assert_eq!(pkg.file_system_memory_footprint, 13405790);
        let python_atom = python.get_atom("python").unwrap();
        assert_eq!(pkg.entry, Some(Cow::Borrowed(python_atom.as_slice())));
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(command_names, &["python"]);
        assert_eq!(commands["python"], python_atom);
    }

    #[test]
    fn parse_a_webc_with_multiple_commands() {
        let pkg = parse_static_webc(COREUTILS.to_vec()).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/coreutils");
        assert_eq!(pkg.version, "1.0.14");
        assert_eq!(pkg.uses, Vec::<String>::new());
        assert_eq!(pkg.envs, HashMap::new());
        assert_eq!(pkg.base_dir, None);
        assert_eq!(pkg.mappings, Vec::<String>::new());
        assert_eq!(pkg.module_memory_footprint, 0);
        assert_eq!(pkg.file_system_memory_footprint, 126);
        assert_eq!(pkg.entry, None);
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(
            command_names,
            &[
                "arch",
                "base32",
                "base64",
                "baseenc",
                "basename",
                "cat",
                "chcon",
                "chgrp",
                "chmod",
                "chown",
                "chroot",
                "cksum",
                "comm",
                "cp",
                "csplit",
                "cut",
                "date",
                "dd",
                "df",
                "dircolors",
                "dirname",
                "du",
                "echo",
                "env",
                "expand",
                "expr",
                "factor",
                "false",
                "fmt",
                "fold",
                "groups",
                "hashsum",
                "head",
                "hostid",
                "hostname",
                "id",
                "install",
                "join",
                "kill",
                "link",
                "ln",
                "logname",
                "ls",
                "mkdir",
                "mkfifo",
                "mknod",
                "mktemp",
                "more",
                "mv",
                "nice",
                "nl",
                "nohup",
                "nproc",
                "numfmt",
                "od",
                "paste",
                "pathchk",
                "pinky",
                "pr",
                "printenv",
                "printf",
                "ptx",
                "pwd",
                "readlink",
                "realpath",
                "relpath",
                "rm",
                "rmdir",
                "runcon",
                "seq",
                "sh",
                "shred",
                "shuf",
                "sleep",
                "sort",
                "split",
                "stat",
                "stdbuf",
                "sum",
                "sync",
                "tac",
                "tail",
                "tee",
                "test",
                "timeout",
                "touch",
                "tr",
                "true",
                "truncate",
                "tsort",
                "tty",
                "uname",
                "unexpand",
                "uniq",
                "unlink",
                "uptime",
                "users",
                "wc",
                "who",
                "whoami",
                "yes",
            ]
        );
    }

    #[test]
    fn parse_a_webc_with_dependencies() {
        let bash = webc::compat::Container::from_bytes(BASH).unwrap();

        let pkg = parse_static_webc(BASH.to_vec()).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/bash");
        assert_eq!(pkg.version, "1.0.12");
        assert_eq!(pkg.uses, &["sharrattj/coreutils@1.0.11"]);
        assert_eq!(pkg.envs, HashMap::new());
        assert_eq!(pkg.base_dir, None);
        assert_eq!(pkg.mappings, Vec::<String>::new());
        assert_eq!(pkg.module_memory_footprint, 0);
        assert_eq!(pkg.file_system_memory_footprint, 0);
        let bash_atom = bash.get_atom("bash").unwrap();
        assert_eq!(pkg.entry, Some(Cow::Borrowed(bash_atom.as_slice())));
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(command_names, &["bash", "sh"]);
    }
}
