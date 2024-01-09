use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use futures::{future::BoxFuture, StreamExt, TryStreamExt};
use once_cell::sync::OnceCell;
use petgraph::visit::EdgeRef;
use virtual_fs::{FileSystem, OverlayFileSystem, WebcVolumeFileSystem};
use webc::{
    compat::{Container, Volume},
    metadata::annotations::Atom as AtomAnnotation,
};

use crate::{
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runtime::{
        package_loader::PackageLoader,
        resolver::{
            DependencyGraph, ItemLocation, PackageId, PackageSummary, Resolution,
            ResolvedFileSystemMapping, ResolvedPackage,
        },
    },
};

/// The maximum number of packages that will be loaded in parallel.
const MAX_PARALLEL_DOWNLOADS: usize = 32;

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
    let commands: Vec<BinaryPackageCommand> =
        commands(&resolution.package.commands, &containers, resolution)?;

    let file_system_memory_footprint = count_file_system(&fs, Path::new("/"));

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
        file_system_memory_footprint,
    };

    Ok(loaded)
}

fn commands(
    commands: &BTreeMap<String, ItemLocation>,
    containers: &HashMap<PackageId, Container>,
    resolution: &Resolution,
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

        if let Some(cmd) =
            load_binary_command(package, name, command_metadata, containers, resolution)?
        {
            pkg_commands.push(cmd);
        }
    }

    Ok(pkg_commands)
}

/// Given a [`webc::metadata::Command`], figure out which atom it uses and load
/// that atom into a [`BinaryPackageCommand`].
fn load_binary_command(
    package_id: &PackageId,
    command_name: &str,
    cmd: &webc::metadata::Command,
    containers: &HashMap<PackageId, Container>,
    resolution: &Resolution,
) -> Result<Option<BinaryPackageCommand>, anyhow::Error> {
    let AtomAnnotation {
        name: atom_name,
        dependency,
        ..
    } = match atom_name_for_command(command_name, cmd)? {
        Some(name) => name,
        None => {
            tracing::warn!(
                cmd.name=command_name,
                cmd.runner=%cmd.runner,
                "Skipping unsupported command",
            );
            return Ok(None);
        }
    };

    let package = &containers[package_id];

    let webc = match dependency {
        Some(dep) => {
            let ix = resolution
                .graph
                .packages()
                .get(package_id)
                .copied()
                .unwrap();
            let graph = resolution.graph.graph();
            let edge_reference = graph
                .edges_directed(ix, petgraph::Direction::Outgoing)
                .find(|edge| edge.weight().alias == dep)
                .with_context(|| format!("Unable to find the \"{dep}\" dependency for the \"{command_name}\" command in \"{package_id}\""))?;

            let other_package = graph.node_weight(edge_reference.target()).unwrap();
            &containers[&other_package.id]
        }
        None => package,
    };

    let atom = webc.get_atom(&atom_name);

    if atom.is_none() && cmd.annotations.is_empty() {
        return Ok(legacy_atom_hack(webc, command_name, cmd));
    }

    let atom = atom.with_context(|| {
        format!(
            "The '{command_name}' command uses the '{atom_name}' atom, but it isn't present in the WEBC file"
        )
    })?;

    let cmd = BinaryPackageCommand::new(command_name.to_string(), cmd.clone(), atom);

    Ok(Some(cmd))
}

fn atom_name_for_command(
    command_name: &str,
    cmd: &webc::metadata::Command,
) -> Result<Option<AtomAnnotation>, anyhow::Error> {
    use webc::metadata::annotations::{EMSCRIPTEN_RUNNER_URI, WASI_RUNNER_URI, WCGI_RUNNER_URI};

    if let Some(atom) = cmd
        .atom()
        .context("Unable to deserialize atom annotations")?
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
        return Ok(Some(AtomAnnotation::new(command_name, None)));
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

    let packages = packages.into_iter().filter_map(|id| {
        let crate::runtime::resolver::Node { pkg, dist, .. } = &graph[&id];
        let summary = PackageSummary {
            pkg: pkg.clone(),
            dist: dist.clone()?,
        };
        Some((id, summary))
    });
    let packages: HashMap<PackageId, Container> = futures::stream::iter(packages)
        .map(|(id, s)| async move {
            match loader.load(&s).await {
                Ok(webc) => Ok((id, webc)),
                Err(e) => Err(e),
            }
        })
        .buffer_unordered(MAX_PARALLEL_DOWNLOADS)
        .try_collect()
        .await?;

    Ok(packages)
}

/// How many bytes worth of files does a directory contain?
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

/// Given a set of [`ResolvedFileSystemMapping`]s and the [`Container`] for each
/// package in a dependency tree, construct the resulting filesystem.
///
/// # Note to future readers
///
/// Sooo... this code is a bit convoluted because we're constrained by the
/// filesystem implementations we've got available.
///
/// Ideally, we would create a WebcVolumeFileSystem for each volume we're
/// using, then we'd have a single "union" filesystem which lets you mount
/// filesystem objects under various paths and can deal with conflicts.
///
/// The OverlayFileSystem lets us make files from multiple filesystem
/// implementations available at the same time, however all of the
/// filesystems will be mounted at "/", when the user wants to mount volumes
/// at arbitrary locations.
///
/// The TmpFileSystem *does* allow mounting at non-root paths, however it can't
/// handle nested paths (e.g. mounting to "/lib" and "/lib/python3.10" - see
/// <https://github.com/wasmerio/wasmer/issues/3678> for more) and you aren't
/// allowed to mount to "/" because it's a special directory that already
/// exists.
///
/// As a result, we'll duct-tape things together and hope for the best 🤞
fn filesystem(
    packages: &HashMap<PackageId, Container>,
    pkg: &ResolvedPackage,
) -> Result<impl FileSystem + Send + Sync, Error> {
    let mut filesystems = Vec::new();
    let mut volumes: HashMap<&PackageId, BTreeMap<String, Volume>> = HashMap::new();

    let mut mountings: Vec<_> = pkg.filesystem.iter().collect();
    mountings.sort_by_key(|m| std::cmp::Reverse(m.mount_path.as_path()));

    for ResolvedFileSystemMapping {
        mount_path,
        volume_name,
        package,
        original_path,
    } in &pkg.filesystem
    {
        // Note: We want to reuse existing Volume instances if we can. That way
        // we can keep the memory usage down. A webc::compat::Volume is
        // reference-counted, anyway.
        let container_volumes = match volumes.entry(package) {
            std::collections::hash_map::Entry::Occupied(entry) => &*entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                // looks like we need to insert it
                let container = packages.get(package)
                    .with_context(|| format!(
                        "\"{}\" wants to use the \"{}\" package, but it isn't in the dependency tree",
                        pkg.root_package,
                        package,
                    ))?;
                &*entry.insert(container.volumes())
            }
        };

        let volume = container_volumes.get(volume_name).with_context(|| {
            format!("The \"{package}\" package doesn't have a \"{volume_name}\" volume")
        })?;

        let original_path = PathBuf::from(original_path);
        let mount_path = mount_path.clone();
        // Get a filesystem which will map "$mount_dir/some-path" to
        // "$original_path/some-path" on the original volume
        let fs =
            MappedPathFileSystem::new(WebcVolumeFileSystem::new(volume.clone()), move |path| {
                let without_mount_dir = path
                    .strip_prefix(&mount_path)
                    .map_err(|_| virtual_fs::FsError::BaseNotDirectory)?;
                let path_on_original_volume = original_path.join(without_mount_dir);
                Ok(path_on_original_volume)
            });

        filesystems.push(fs);
    }

    let fs = OverlayFileSystem::new(virtual_fs::EmptyFileSystem::default(), filesystems);

    Ok(fs)
}

/// A [`FileSystem`] implementation that lets you map the [`Path`] to something
/// else.
#[derive(Clone, PartialEq)]
struct MappedPathFileSystem<F, M> {
    inner: F,
    map: M,
}

impl<F, M> MappedPathFileSystem<F, M>
where
    M: Fn(&Path) -> Result<PathBuf, virtual_fs::FsError> + Send + Sync + 'static,
{
    fn new(inner: F, map: M) -> Self {
        MappedPathFileSystem { inner, map }
    }

    fn path(&self, path: &Path) -> Result<PathBuf, virtual_fs::FsError> {
        let path = (self.map)(path)?;

        // Don't forget to make the path absolute again.
        Ok(Path::new("/").join(path))
    }
}

impl<M, F> FileSystem for MappedPathFileSystem<F, M>
where
    F: FileSystem,
    M: Fn(&Path) -> Result<PathBuf, virtual_fs::FsError> + Send + Sync + 'static,
{
    fn read_dir(&self, path: &Path) -> virtual_fs::Result<virtual_fs::ReadDir> {
        let path = self.path(path)?;
        self.inner.read_dir(&path)
    }

    fn create_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        let path = self.path(path)?;
        self.inner.create_dir(&path)
    }

    fn remove_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        let path = self.path(path)?;
        self.inner.remove_dir(&path)
    }

    fn rename<'a>(&'a self, from: &Path, to: &Path) -> BoxFuture<'a, virtual_fs::Result<()>> {
        let from = from.to_owned();
        let to = to.to_owned();
        Box::pin(async move {
            let from = self.path(&from)?;
            let to = self.path(&to)?;
            self.inner.rename(&from, &to).await
        })
    }

    fn metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        let path = self.path(path)?;
        self.inner.metadata(&path)
    }

    fn remove_file(&self, path: &Path) -> virtual_fs::Result<()> {
        let path = self.path(path)?;
        self.inner.remove_file(&path)
    }

    fn new_open_options(&self) -> virtual_fs::OpenOptions {
        virtual_fs::OpenOptions::new(self)
    }
}

impl<F, M> virtual_fs::FileOpener for MappedPathFileSystem<F, M>
where
    F: FileSystem,
    M: Fn(&Path) -> Result<PathBuf, virtual_fs::FsError> + Send + Sync + 'static,
{
    fn open(
        &self,
        path: &Path,
        conf: &virtual_fs::OpenOptionsConfig,
    ) -> virtual_fs::Result<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>> {
        let path = self.path(path)?;
        self.inner
            .new_open_options()
            .options(conf.clone())
            .open(path)
    }
}

impl<F, M> Debug for MappedPathFileSystem<F, M>
where
    F: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MappedPathFileSystem")
            .field("inner", &self.inner)
            .field("map", &std::any::type_name::<M>())
            .finish()
    }
}
