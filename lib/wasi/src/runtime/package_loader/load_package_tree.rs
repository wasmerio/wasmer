use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Error};
use futures::{stream::FuturesUnordered, TryStreamExt};
use once_cell::sync::OnceCell;
use virtual_fs::{FileSystem, WebcVolumeFileSystem};
use webc::compat::Container;

use crate::{
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runtime::{
        package_loader::PackageLoader,
        resolver::{
            DependencyGraph, ItemLocation, PackageId, Resolution, ResolvedPackage, Summary,
        },
    },
};

/// Given a fully resolved package, load it into memory for execution.
#[tracing::instrument(level = "debug", skip_all)]
pub async fn load_package_tree(
    root: &Container,
    loader: &dyn PackageLoader,
    resolution: &Resolution,
) -> Result<BinaryPackage, Error> {
    let mut containers = fetch_dependencies(loader, &resolution.package, &resolution.graph).await?;
    containers.insert(resolution.package.root_package.clone(), root.clone());
    let fs = filesystem(&containers, &resolution.package)?;

    let root = &resolution.package.root_package;
    let commands: Vec<BinaryPackageCommand> = commands(&resolution.package.commands, &containers)?;

    let file_system_memory_footprint = count_file_system(&fs, Path::new("/"));
    let atoms_in_use: HashSet<_> = commands.iter().map(|cmd| cmd.atom()).collect();
    let module_memory_footprint = atoms_in_use
        .iter()
        .fold(0, |footprint, atom| footprint + atom.len() as u64);

    let loaded = BinaryPackage {
        package_name: root.package_name.clone(),
        version: root.version.clone(),
        when_cached: crate::syscalls::platform_clock_time_get(
            wasmer_wasix_types::wasi::Snapshot0Clockid::Monotonic,
            1_000_000,
        )
        .ok()
        .map(|ts| ts as u128),
        hash: OnceCell::new(),
        entrypoint_cmd: resolution.package.entrypoint.clone(),
        webc_fs: Arc::new(fs),
        commands,
        uses: Vec::new(),
        module_memory_footprint,
        file_system_memory_footprint,
    };

    Ok(loaded)
}

fn commands(
    commands: &BTreeMap<String, ItemLocation>,
    containers: &HashMap<PackageId, Container>,
) -> Result<Vec<BinaryPackageCommand>, Error> {
    let mut pkg_commands = Vec::new();

    for (
        name,
        ItemLocation {
            name: original_name,
            package,
        },
    ) in commands
    {
        let webc = &containers[package];
        let manifest = webc.manifest();
        let command_metadata = &manifest.commands[original_name];

        if let Some(cmd) = load_binary_command(webc, name, command_metadata)? {
            pkg_commands.push(cmd);
        }
    }

    Ok(pkg_commands)
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
        return Ok(legacy_atom_hack(webc, name, cmd));
    }

    let atom = atom
        .with_context(|| format!("The '{name}' command uses the '{atom_name}' atom, but it isn't present in the WEBC file"))?;

    let cmd = BinaryPackageCommand::new(name.to_string(), cmd.clone(), atom);

    Ok(Some(cmd))
}

fn atom_name_for_command(
    command_name: &str,
    cmd: &webc::metadata::Command,
) -> Result<Option<String>, anyhow::Error> {
    use webc::metadata::annotations::{
        Emscripten, Wasi, EMSCRIPTEN_RUNNER_URI, WASI_RUNNER_URI, WCGI_RUNNER_URI,
    };

    // FIXME: command metadata should include an "atom: Option<String>" field
    // because it's so common, rather than relying on each runner to include
    // annotations where "atom" just so happens to contain the atom's name
    // (like in Wasi and Emscripten)

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
fn legacy_atom_hack(
    webc: &Container,
    command_name: &str,
    metadata: &webc::metadata::Command,
) -> Option<BinaryPackageCommand> {
    let (name, atom) = webc.atoms().into_iter().next()?;

    tracing::debug!(
        command_name,
        atom.name = name.as_str(),
        atom.len = atom.len(),
        "(hack) The command metadata is malformed. Falling back to the first atom in the WEBC file",
    );

    Some(BinaryPackageCommand::new(
        command_name.to_string(),
        metadata.clone(),
        atom,
    ))
}

async fn fetch_dependencies(
    loader: &dyn PackageLoader,
    pkg: &ResolvedPackage,
    graph: &DependencyGraph,
) -> Result<HashMap<PackageId, Container>, Error> {
    let mut packages = HashSet::new();

    for loc in pkg.commands.values() {
        packages.insert(loc.package.clone());
    }

    for mapping in &pkg.filesystem {
        packages.insert(mapping.package.clone());
    }

    // We don't need to download the root package
    packages.remove(&pkg.root_package);

    let packages: FuturesUnordered<_> = packages
        .into_iter()
        .map(|id| async {
            let summary = Summary {
                pkg: graph.package_info[&id].clone(),
                dist: graph.distribution[&id].clone(),
            };
            loader.load(&summary).await.map(|webc| (id, webc))
        })
        .collect();

    let packages: HashMap<PackageId, Container> = packages.try_collect().await?;

    Ok(packages)
}

fn filesystem(
    packages: &HashMap<PackageId, Container>,
    pkg: &ResolvedPackage,
) -> Result<impl FileSystem + Send + Sync, Error> {
    // FIXME: Take the [fs] table into account
    // See <https://github.com/wasmerio/wasmer/issues/3744> for more
    let root = &packages[&pkg.root_package];
    let fs = WebcVolumeFileSystem::mount_all(root);
    Ok(fs)
}

fn count_file_system(fs: &dyn FileSystem, path: &Path) -> u64 {
    let mut total = 0;

    let dir = match fs.read_dir(path) {
        Ok(d) => d,
        Err(_err) => {
            return 0;
        }
    };

    for entry in dir.flatten() {
        if let Ok(meta) = entry.metadata() {
            total += meta.len();
            if meta.is_dir() {
                total += count_file_system(fs, entry.path.as_path());
            }
        }
    }

    total
}
