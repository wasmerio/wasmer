use anyhow::Context;
use once_cell::sync::OnceCell;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
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

use crate::bin_factory::{BinaryPackage, BinaryPackageCommand};

pub fn parse_static_webc(data: Vec<u8>) -> Result<BinaryPackage, anyhow::Error> {
    let webc = Container::from_bytes(data)?;
    parse_webc(&webc).with_context(|| "Could not parse webc".to_string())
}

pub(crate) fn parse_webc(webc: &Container) -> Result<BinaryPackage, anyhow::Error> {
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
        hash: OnceCell::new(),
        webc_fs: Arc::new(webc_fs),
        commands: Arc::new(RwLock::new(commands.into_values().collect())),
        uses,
        version: wapm.version.parse()?,
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
    const COREUTILS: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc");
    const BASH: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc");
    const HELLO: &[u8] = include_bytes!("../../../../tests/integration/cli/tests/webc/hello-0.1.0-665d2ddc-80e6-4845-85d3-4587b1693bb7.webc");

    #[test]
    fn parse_the_python_webc_file() {
        let python = webc::compat::Container::from_bytes(PYTHON).unwrap();

        let pkg = parse_webc(&python).unwrap();

        assert_eq!(pkg.package_name, "python");
        assert_eq!(pkg.version.to_string(), "0.1.0");
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

        let pkg = parse_webc(&coreutils).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/coreutils");
        assert_eq!(pkg.version.to_string(), "1.0.16");
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

        let pkg = parse_webc(&bash).unwrap();

        assert_eq!(pkg.package_name, "sharrattj/bash");
        assert_eq!(pkg.version.to_string(), "1.0.16");
        assert_eq!(pkg.uses, &["sharrattj/coreutils@1.0.16"]);
        assert_eq!(pkg.module_memory_footprint, 1847052);
        assert_eq!(pkg.file_system_memory_footprint, 0);
        let commands = pkg.commands.read().unwrap();
        let commands: BTreeMap<&str, &[u8]> = commands
            .iter()
            .map(|cmd| (cmd.name(), cmd.atom()))
            .collect();
        let command_names: Vec<_> = commands.keys().copied().collect();
        assert_eq!(command_names, &["bash"]);
        assert_eq!(commands["bash"], bash.get_atom("bash").unwrap());
    }

    #[test]
    fn parse_a_webc_with_dependencies_and_no_commands() {
        let pkg = parse_static_webc(HELLO.to_vec()).unwrap();

        assert_eq!(pkg.package_name, "wasmer/hello");
        assert_eq!(pkg.version.to_string(), "0.1.0");
        let commands = pkg.commands.read().unwrap();
        assert!(commands.is_empty());
        assert!(pkg.entry.is_none());
        assert_eq!(pkg.uses, ["sharrattj/static-web-server@1"]);
    }
}
