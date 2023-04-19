use anyhow::{bail, Context};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, RwLock},
};
use virtual_fs::{FileSystem, WebcVolumeFileSystem};
use wasmer_wasix_types::wasi::Snapshot0Clockid;

use webc::{
    metadata::{
        annotations::{EMSCRIPTEN_RUNNER_URI, WASI_RUNNER_URI, WCGI_RUNNER_URI},
        UrlOrManifest,
    },
    Container,
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
    tracing::debug!("request: {}", url_query);

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
    tracing::debug!("response: {:?}", data);

    let PiritaVersionedDownload {
        url: download_url,
        version,
    } = wapm_extract_version(&data).context("No pirita download URL available")?;
    let mut pkg = download_webc(cache_dir, name, download_url, client).await?;
    pkg.version = version;
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
    let webc = Container::from_bytes(data)?;
    parse_webc_v2(&webc).with_context(|| "Could not parse webc".to_string())
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

    // fast path
    let path = compute_path(cache_dir, name);

    #[cfg(feature = "sys")]
    if path.exists() {
        tracing::debug!(path=%path.display(), "Parsing cached WEBC file");

        match Container::from_disk(&path) {
            Ok(webc) => {
                return parse_webc_v2(&webc)
                    .with_context(|| format!("Could not parse webc at path '{}'", path.display()));
            }
            Err(err) => {
                tracing::warn!(
                    error = &err as &dyn std::error::Error,
                    "failed to parse WEBC",
                );
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
            tracing::debug!(
                "failed to write webc cache file [{}] - {}",
                temp_path.as_path().to_string_lossy(),
                err
            );
        }
        if let Err(err) = std::fs::rename(temp_path.as_path(), path.as_path()) {
            tracing::debug!(
                "failed to rename webc cache file [{}] - {}",
                temp_path.as_path().to_string_lossy(),
                err
            );
        }

        match Container::from_disk(&path) {
            Ok(webc) => {
                return parse_webc_v2(&webc)
                    .with_context(|| format!("Could not parse webc at path '{}'", path.display()))
            }
            Err(e) => {
                tracing::warn!(
                    path=%temp_path.display(),
                    error=&e as &dyn std::error::Error,
                    "Unable to parse temporary WEBC from disk",
                )
            }
        }
    }

    let webc = Container::from_bytes(data)
        .with_context(|| format!("Failed to parse downloaded from '{pirita_download_url}'"))?;
    let package = parse_webc_v2(&webc).context("Could not parse binary package")?;

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

fn parse_webc_v2(webc: &Container) -> Result<BinaryPackage, anyhow::Error> {
    let manifest = webc.manifest();

    let wapm: webc::metadata::annotations::Wapm = manifest
        .package_annotation("wapm")?
        .context("The package must have 'wapm' annotations")?;

    let mut commands = HashMap::new();

    for (name, cmd) in &manifest.commands {
        if let Some(cmd) = load_binary_command(webc, name, cmd)? {
            commands.insert(name.as_str(), cmd);
        }
    }

    let entry = manifest.entrypoint.as_deref().and_then(|entry| {
        let cmd = commands.get(entry)?;
        Some(cmd.atom.clone())
    });

    let webc_fs = WebcVolumeFileSystem::mount_all(webc);

    // List all the dependencies
    let uses: Vec<_> = manifest
        .use_map
        .values()
        .filter_map(|uses| match uses {
            UrlOrManifest::Url(url) => Some(url.path()),
            UrlOrManifest::Manifest(manifest) => manifest.origin.as_deref(),
            UrlOrManifest::RegistryDependentUrl(url) => Some(url),
        })
        .map(String::from)
        .collect();

    let module_memory_footprint = entry.as_deref().map(|b| b.len() as u64).unwrap_or(0);
    let file_system_memory_footprint = count_file_system(&webc_fs, Path::new("/"));

    let pkg = BinaryPackage {
        package_name: wapm.name,
        when_cached: Some(
            crate::syscalls::platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000)
                .unwrap() as u128,
        ),
        entry: entry.map(Into::into),
        hash: Arc::new(Mutex::new(None)),
        webc_fs: Some(Arc::new(webc_fs)),
        commands: Arc::new(RwLock::new(commands.into_values().collect())),
        uses,
        version: wapm.version,
        module_memory_footprint,
        file_system_memory_footprint,
    };

    Ok(pkg)
}

fn load_binary_command(
    webc: &Container,
    name: &str,
    cmd: &webc::metadata::Command,
) -> Result<Option<BinaryPackageCommand>, anyhow::Error> {
    let atom_name = match atom_name_for_command(name, cmd)? {
        Some(name) => name,
        None => {
            tracing::warn!(
                cmd.name=name,
                cmd.runner=%cmd.runner,
                "Skipping unsupported command",
            );
            return Ok(None);
        }
    };

    let atom = webc.get_atom(&atom_name);

    if atom.is_none() && cmd.annotations.is_empty() {
        return Ok(legacy_atom_hack(webc, name));
    }

    let atom = atom
        .with_context(|| format!("The '{name}' command uses the '{atom_name}' atom, but it isn't present in the WEBC file"))?;

    let cmd = BinaryPackageCommand::new(name.to_string(), atom);

    Ok(Some(cmd))
}

fn atom_name_for_command(
    command_name: &str,
    cmd: &webc::metadata::Command,
) -> Result<Option<String>, anyhow::Error> {
    use webc::metadata::annotations::{Emscripten, Wasi};

    if let Some(Wasi { atom, .. }) = cmd
        .annotation("wasi")
        .context("Unable to deserialize 'wasi' annotations")?
    {
        return Ok(Some(atom));
    }

    if let Some(Emscripten {
        atom: Some(atom), ..
    }) = cmd
        .annotation("emscripten")
        .context("Unable to deserialize 'emscripten' annotations")?
    {
        return Ok(Some(atom));
    }

    if [WASI_RUNNER_URI, WCGI_RUNNER_URI, EMSCRIPTEN_RUNNER_URI]
        .iter()
        .any(|uri| cmd.runner.starts_with(uri))
    {
        // Note: We use the command name as the atom name as a special case
        // for known runner types because sometimes people will construct
        // a manifest by hand instead of using wapm2pirita.
        tracing::debug!(
            command = command_name,
            "No annotations specifying the atom name found. Falling back to the command name"
        );
        return Ok(Some(command_name.to_string()));
    }

    Ok(None)
}

/// HACK: Some older packages like `sharrattj/bash` and `sharrattj/coreutils`
/// contain commands with no annotations. When this happens, you can just assume
/// it wants to use the first atom in the WEBC file.
///
/// That works because most of these packages only have a single atom (e.g. in
/// `sharrattj/coreutils` there are commands for `ls`, `pwd`, and so on, but
/// under the hood they all use the `coreutils` atom).
///
/// See <https://github.com/wasmerio/wasmer/commit/258903140680716da1431d92bced67d486865aeb>
/// for more.
fn legacy_atom_hack(webc: &Container, command_name: &str) -> Option<BinaryPackageCommand> {
    let (name, atom) = webc.atoms().into_iter().next()?;

    tracing::debug!(
        command_name,
        atom.name = name.as_str(),
        atom.len = atom.len(),
        "(hack) The command metadata is malformed. Falling back to the first atom in the WEBC file",
    );

    Some(BinaryPackageCommand::new(command_name.to_string(), atom))
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
    use std::collections::BTreeMap;

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");
    const COREUTILS: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/coreutils-1.0.14-076508e5-e704-463f-b467-f3d9658fc907.webc");
    const BASH: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/bash-1.0.12-0103d733-1afb-4a56-b0ef-0e124139e996.webc");
    const HELLO: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/hello-0.1.0-665d2ddc-80e6-4845-85d3-4587b1693bb7.webc");

    #[test]
    fn parse_the_python_webc_file() {
        let python = webc::compat::Container::from_bytes(PYTHON).unwrap();

        let pkg = parse_webc_v2(&python).unwrap();

        assert_eq!(pkg.package_name, "python");
        assert_eq!(pkg.version, "0.1.0");
        assert_eq!(pkg.uses, Vec::<String>::new());
        assert_eq!(pkg.module_memory_footprint, 4694941);
        assert_eq!(pkg.file_system_memory_footprint, 13387764);
        let python_atom = python.get_atom("python").unwrap();
        assert_eq!(pkg.entry.as_deref(), Some(python_atom.as_slice()));
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(command_names, &["python"]);
        assert_eq!(commands["python"], python_atom);

        // Note: It's important that the entry we parse doesn't allocate, so
        // make sure it lies within the original PYTHON buffer.
        let bounds = PYTHON.as_ptr_range();

        let entry_ptr = pkg.entry.as_deref().unwrap().as_ptr();
        assert!(bounds.start <= entry_ptr && entry_ptr < bounds.end);

        let python_cmd_ptr = commands["python"].as_ptr();
        assert!(bounds.start <= python_cmd_ptr && python_cmd_ptr < bounds.end);
    }

    #[test]
    fn parse_a_webc_with_multiple_commands() {
        let coreutils = Container::from_bytes(COREUTILS).unwrap();

        let pkg = parse_webc_v2(&coreutils).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/coreutils");
        assert_eq!(pkg.version, "1.0.14");
        assert_eq!(pkg.uses, Vec::<String>::new());
        assert_eq!(pkg.module_memory_footprint, 0);
        assert_eq!(pkg.file_system_memory_footprint, 44);
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
        let coreutils_atom = coreutils.get_atom("coreutils").unwrap();
        for (cmd, atom) in commands {
            assert_eq!(atom.len(), coreutils_atom.len(), "{cmd}");
            assert_eq!(atom, coreutils_atom, "{cmd}");
        }
    }

    #[test]
    fn parse_a_webc_with_dependencies() {
        let bash = webc::compat::Container::from_bytes(BASH).unwrap();

        let pkg = parse_webc_v2(&bash).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/bash");
        assert_eq!(pkg.version, "1.0.12");
        assert_eq!(pkg.uses, &["sharrattj/coreutils@1.0.11"]);
        assert_eq!(pkg.module_memory_footprint, 0);
        assert_eq!(pkg.file_system_memory_footprint, 0);
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(command_names, &["bash", "sh"]);
        assert_eq!(commands["bash"], bash.get_atom("bash").unwrap());
        assert_eq!(commands["sh"], commands["bash"]);
    }

    #[test]
    fn parse_a_webc_with_dependencies_and_no_commands() {
        let pkg = parse_static_webc(HELLO.to_vec()).unwrap();

        assert_eq!(pkg.package_name, "wasmer/hello");
        assert_eq!(pkg.version, "0.1.0");
        let commands = pkg.commands.read().unwrap();
        assert!(commands.is_empty());
        assert!(pkg.entry.is_none());
        assert_eq!(pkg.uses, ["sharrattj/static-web-server@1"]);
    }
}
