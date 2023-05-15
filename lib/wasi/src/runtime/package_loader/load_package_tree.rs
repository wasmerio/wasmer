use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::{Arc, RwLock},
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
        resolver::{ItemLocation, PackageId, Resolution, ResolvedPackage, Summary},
    },
};

/// Given a fully resolved package, load it into memory for execution.
pub async fn load_package_tree(
    loader: &impl PackageLoader,
    resolution: &Resolution,
) -> Result<BinaryPackage, Error> {
    let containers =
        used_packages(loader, &resolution.package, &resolution.graph.summaries).await?;
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
        entry: resolution.package.entrypoint.as_deref().and_then(|entry| {
            commands
                .iter()
                .find(|cmd| cmd.name() == entry)
                .map(|cmd| cmd.atom.clone())
        }),
        webc_fs: Some(Arc::new(fs)),
        commands: Arc::new(RwLock::new(commands)),
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
        let cmd = &manifest.commands[original_name];
        let atom_name = infer_atom_name(cmd).with_context(|| {
            format!(
                "Unable to infer the atom name for the \"{original_name}\" command in {package}"
            )
        })?;
        let atom = webc.get_atom(&atom_name).with_context(|| {
            format!("The {package} package doesn't contain a \"{atom_name}\" atom")
        })?;
        pkg_commands.push(BinaryPackageCommand::new(name.clone(), atom));
    }

    Ok(pkg_commands)
}

fn infer_atom_name(cmd: &webc::metadata::Command) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Annotation {
        atom: String,
    }

    // FIXME: command metadata should include an "atom: Option<String>" field
    // because it's so common, rather than relying on each runner to include
    // annotations where "atom" just so happens to contain the atom's name
    // (like in Wasi and Emscripten)

    for annotation in cmd.annotations.values() {
        if let Ok(Annotation { atom: atom_name }) =
            serde_cbor::value::from_value(annotation.clone())
        {
            return Some(atom_name);
        }
    }

    None
}

async fn used_packages(
    loader: &impl PackageLoader,
    pkg: &ResolvedPackage,
    summaries: &HashMap<PackageId, Summary>,
) -> Result<HashMap<PackageId, Container>, Error> {
    let mut packages = HashSet::new();
    packages.insert(pkg.root_package.clone());

    for loc in pkg.commands.values() {
        packages.insert(loc.package.clone());
    }

    for mapping in &pkg.filesystem {
        packages.insert(mapping.package.clone());
    }

    let packages: FuturesUnordered<_> = packages
        .into_iter()
        .map(|id| async { loader.load(&summaries[&id]).await.map(|webc| (id, webc)) })
        .collect();

    let packages: HashMap<PackageId, Container> = packages.try_collect().await?;

    Ok(packages)
}

fn filesystem(
    packages: &HashMap<PackageId, Container>,
    pkg: &ResolvedPackage,
) -> Result<impl FileSystem + Send + Sync, Error> {
    // TODO: Take the [fs] table into account
    let root = &packages[&pkg.root_package];
    let fs = WebcVolumeFileSystem::mount_all(root);
    Ok(fs)
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
