//! Runs programs on the WASIX v2 kernel (the `wasixv2` crate's [`Machine`]): WASIX v2 modules,
//! WASIX v1 and WASI preview1 modules through the kernel's v1 compatibility ABI, and packages
//! whose command runs one of them.

use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Error, bail};
use wasixv2::{
    CommandDefaults, Console, Credentials, Image, ImageId, Layer, LoadError, Loader, LocalFuture,
    Machine, OwnedBuffer, ProcessConfig, Signaller, Stdio, Uname,
};
use wasmer::{Engine, Module};
use wasmer_config::package::{PackageId, PackageSource};
use wasmer_types::{ModuleHash, target::Target};
use wasmer_wasix::{
    Runtime,
    bin_factory::BinaryPackage,
    runners::MAPPED_CURRENT_DIR_DEFAULT_PATH,
    runtime::{module_cache::HashedModuleData, task_manager::VirtualTaskManagerExt as _},
};
use webc::{
    Container,
    metadata::annotations::{WASI_RUNNER_URI, Wasi as WasiAnnotation},
    v3::read::OwnedReader,
};

#[cfg(unix)]
use super::terminal::{self, HostTerminal};
use super::{
    CliPackageSource, GuestExit, Run, exit_with_wasi_exit_code,
    runtime::{MonitoringRuntime, PackageTree},
    wasi::utf8_env_part,
};
use crate::backend::{BackendType, RuntimeOptions};

/// What to start on the machine.
pub(super) struct MachineProgram {
    argv0: String,
    start: Start,
    /// The command's `wasi` annotation, for a package.
    wasi: Option<WasiAnnotation>,
    /// The package volumes to mount, each over the ones before it.
    volumes: Vec<PackageVolume>,
    /// The package tree's commands, for `/bin` and `/usr/bin`.
    commands: Vec<PackageCommand>,
    /// A host directory to map read-only to [`SCRIPT_DIR`]: that of a host script no
    /// `--volume` holds.
    script_dir: Option<PathBuf>,
}

/// Where `wasmer run` maps the directory of a host script outside every `--volume`, so its
/// interpreter can read it.
const SCRIPT_DIR: &str = "/run/wasmer/script";

/// How the program's process starts.
enum Start {
    /// A module compiled on the host, run as it is.
    Module { engine: Engine, module: Module },
    /// The guest file at this path, which the process `execve`s: a package command or a
    /// script.
    File(String),
}

/// A command of the package tree: `/bin/<name>` and `/usr/bin/<name>` hold its atom.
struct PackageCommand {
    name: String,
    atom: OwnedBuffer,
    /// What WASIX v1 programs that start it pass along, from its `wasi` annotation.
    defaults: CommandDefaults,
}

/// A volume of a package in the tree, and where it goes.
struct PackageVolume {
    reader: Arc<OwnedReader>,
    name: String,
    /// An absolute guest path; `/` makes the volume a layer of the root filesystem.
    mount_path: String,
}

impl MachineProgram {
    pub(super) fn module(path: &Path, engine: Engine, module: Module) -> Self {
        Self {
            argv0: path.display().to_string(),
            start: Start::Module { engine, module },
            wasi: None,
            volumes: Vec::new(),
            commands: Vec::new(),
            script_dir: None,
        }
    }

    /// The `#!` script at `guest`, a guest path, which `execve` hands to its interpreter, and
    /// the host directory to map to [`SCRIPT_DIR`] for it, if any.
    pub(super) fn script((guest, script_dir): (String, Option<PathBuf>)) -> Self {
        Self {
            argv0: guest.clone(),
            start: Start::File(guest),
            wasi: None,
            volumes: Vec::new(),
            commands: Vec::new(),
            script_dir,
        }
    }

    fn is_package(&self) -> bool {
        self.wasi.is_some()
    }

    /// Of the guest paths `mounts` lists, the last that covers the directory of the package's
    /// command (`/bin`), which then holds whatever that mount has there.
    fn mount_hiding_the_command(&self, mounts: impl Iterator<Item = String>) -> Option<String> {
        let (Start::File(path), true) = (&self.start, self.is_package()) else {
            return None;
        };
        let dir = Path::new(path).parent()?;
        mounts
            .filter(|mount| Path::new(mount) != Path::new("/") && dir.starts_with(mount))
            .last()
    }

    /// Adds the packages `--use` names, in order, under the program's own: their commands
    /// after the program's package's, so a name that has stays its, and their volumes before
    /// its volumes, which obscure them.
    pub(super) fn using(mut self, used: Vec<UsedPackage>) -> Self {
        let mut volumes = Vec::new();
        for package in used {
            volumes.extend(package.volumes);
            self.commands.extend(package.commands);
        }
        volumes.append(&mut self.volumes);
        // A stable sort keeps the obscuring order among mounts at the same depth.
        volumes.sort_by_key(|v| Path::new(&v.mount_path).components().count());
        self.volumes = volumes;
        self
    }
}

/// A package `--use` names: its tree's commands and volumes.
pub(super) struct UsedPackage {
    volumes: Vec<PackageVolume>,
    commands: Vec<PackageCommand>,
}

/// Whether the host file at `path` is a `#!` script, which runs as its interpreter on the
/// machine.
pub(super) fn is_script(path: &Path) -> bool {
    let mut magic = [0; 2];
    std::fs::File::open(path)
        .and_then(|mut file| io::Read::read_exact(&mut file, &mut magic))
        .is_ok_and(|()| magic == *b"#!")
}

/// Compiles the images the machine's processes `execve`, and the program `wasmer run` starts,
/// as `wasmer run` compiles a module: on a backend with the WebAssembly features the image
/// uses, with the machine's preemption points ([`wasixv2::instrument`]), through the runtime's
/// compiled-module cache. Each image is instrumented and compiled once per run: later loads of
/// the same image find the module by the image's identity (a package file's buffer) or, for a
/// copy, by a hash of its bytes, without looking at them again.
pub(super) struct CachedLoader {
    runtime: Arc<dyn Runtime + Send + Sync>,
    options: RuntimeOptions,
    /// One engine per backend: the one `wasmer run` picked, and others made when an image
    /// first needs them.
    engines: Mutex<Vec<(BackendType, Engine)>>,
    /// What each image compiled to.
    modules: Mutex<HashMap<ImageKey, Loaded>>,
}

/// What tells an image apart in [`CachedLoader`]'s modules.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ImageKey {
    Shared(ImageId),
    Bytes(ModuleHash),
}

struct Loaded {
    engine: Engine,
    module: Module,
    /// The bytes of a [`ImageKey::Shared`] image, which keep its identity theirs.
    _image: Option<Image>,
}

impl CachedLoader {
    /// A loader that compiles on `engine`, the backend `wasmer run` picked, whatever needs
    /// no other.
    pub(super) fn new(
        runtime: Arc<dyn Runtime + Send + Sync>,
        options: RuntimeOptions,
        engine: (BackendType, Engine),
    ) -> Self {
        Self {
            runtime,
            options,
            engines: Mutex::new(vec![engine]),
            modules: Mutex::default(),
        }
    }

    /// The engine of the backend [`RuntimeOptions::get_engine_for_module`] picks for `wasm`.
    fn engine_for(&self, wasm: &[u8]) -> Result<Engine, Error> {
        let target = Target::default();
        let features = self
            .options
            .detect_features_from_wasm(wasm)
            .unwrap_or_default();
        let backend = self.options.get_backend_for_features(&features, &target)?;
        let mut engines = self.engines.lock().unwrap();
        if let Some((_, engine)) = engines.iter().find(|(b, _)| *b == backend) {
            return Ok(engine.clone());
        }
        let engine = backend.get_engine(&target, &self.options)?;
        engines.push((backend, engine.clone()));
        Ok(engine)
    }

    async fn compile(&self, wasm: &[u8]) -> Result<(Engine, Module), Error> {
        let engine = self.engine_for(wasm)?;
        let (instrumented, failed) = match wasixv2::instrument(wasm) {
            Ok(instrumented) => (instrumented, None),
            Err(err) => (wasm.to_vec(), Some(err)),
        };
        let wasm = HashedModuleData::new(instrumented);
        let module = self.runtime.load_hashed_module(wasm, Some(&engine)).await?;
        if let Some(err) = failed {
            wasixv2::warn_not_preemptible(&err);
        }
        Ok((engine, module))
    }
}

impl Loader for CachedLoader {
    fn load<'a>(
        &'a self,
        _engine: &'a Engine,
        image: &'a Image,
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        Box::pin(async move {
            let key = match image.identity() {
                Some(id) => ImageKey::Shared(id),
                None => ImageKey::Bytes(ModuleHash::new(image.bytes())),
            };
            if let Some(loaded) = self.modules.lock().unwrap().get(&key) {
                return Ok((loaded.engine.clone(), loaded.module.clone()));
            }
            let (engine, module) = self
                .compile(image.bytes())
                .await
                .map_err(LoadError::other)?;
            let loaded = Loaded {
                engine: engine.clone(),
                module: module.clone(),
                _image: matches!(key, ImageKey::Shared(_)).then(|| image.clone()),
            };
            self.modules.lock().unwrap().insert(key, loaded);
            Ok((engine, module))
        })
    }
}

/// The commands of the package tree, in the order the resolver gave them: it made each name
/// that of the first package in the tree (the root, then its dependencies) that has it.
///
/// Every command becomes a file, whatever its runner, as v1 installed them: running one is
/// `execve` of the file, which starts what some ABI serves and fails with `ENOEXEC` for
/// anything else, as Linux's binary formats decide. A WCGI command's atom is a WASI program
/// and runs as one, as a CGI program run by hand does; a WASM-4 cartridge is `ENOEXEC`. Only
/// the program `wasmer run` starts itself must have the WASI runner.
fn package_commands(pkg: &BinaryPackage) -> Result<Vec<PackageCommand>, Error> {
    pkg.commands
        .iter()
        .map(|cmd| {
            let wasi = cmd
                .metadata()
                .annotation::<WasiAnnotation>(WasiAnnotation::KEY)
                .with_context(|| format!("invalid annotations on the \"{}\" command", cmd.name()))?
                .unwrap_or_else(|| WasiAnnotation::new(cmd.name()));
            Ok(PackageCommand {
                name: cmd.name().to_string(),
                atom: cmd.atom(),
                defaults: CommandDefaults {
                    argv0: wasi.exec_name,
                    args: wasi.main_args.unwrap_or_default(),
                    env: wasi.env.unwrap_or_default(),
                },
            })
        })
        .collect()
}

/// Whether the machine runs `module`, a precompiled module: one importing WASIX v2, or WASIX v1
/// or WASI preview1. A module with neither is plain WebAssembly; a v1 flavour the kernel
/// cannot serve is an error.
pub(super) fn runs_on_machine(module: &Module) -> Result<bool, Error> {
    if wasixv2::targets_wasixv2(module) {
        return Ok(true);
    }
    match wasixv2::wasix1_module_flavor(module) {
        None => Ok(false),
        Some(flavor) => match flavor.unsupported_reason() {
            None => Ok(true),
            Some(reason) => bail!("{reason}"),
        },
    }
}

/// [`runs_on_machine`] for `wasm`, a module not compiled yet, from its imports alone.
pub(super) fn wasm_runs_on_machine(wasm: &[u8]) -> Result<bool, Error> {
    if wasixv2::is_wasixv2_module(wasm) {
        return Ok(true);
    }
    match wasixv2::is_wasix1_module(wasm) {
        None => Ok(false),
        Some(flavor) => match flavor.unsupported_reason() {
            None => Ok(true),
            Some(reason) => bail!("{reason}"),
        },
    }
}

impl Run {
    /// The program to run for the package's selected command, chosen as `--entrypoint` or the
    /// package's entrypoint says. `tree` is the package tree `pkg` was loaded from.
    pub(super) fn machine_program(
        &self,
        pkg: &BinaryPackage,
        tree: &PackageTree,
    ) -> Result<MachineProgram, Error> {
        let name = match self.entrypoint.as_deref() {
            Some(name) => name,
            None => pkg.infer_entrypoint()?,
        };
        let cmd = pkg
            .get_command(name)
            .with_context(|| format!("Unable to get metadata for the \"{name}\" command"))?;
        if !cmd.metadata().runner.starts_with(WASI_RUNNER_URI) {
            bail!(
                "Unable to find a runner that supports \"{}\"",
                cmd.metadata().runner
            );
        }
        let wasi = cmd
            .metadata()
            .annotation::<WasiAnnotation>(WasiAnnotation::KEY)?
            .unwrap_or_else(|| WasiAnnotation::new(name));
        let file = match &self.input {
            CliPackageSource::File(path) => Some(path.as_path()),
            _ => None,
        };
        Ok(MachineProgram {
            argv0: wasi.exec_name.clone().unwrap_or_else(|| name.to_string()),
            start: Start::File(format!("/bin/{name}")),
            wasi: Some(wasi),
            volumes: self.package_volumes(tree, file)?,
            commands: package_commands(pkg)?,
            script_dir: None,
        })
    }

    /// The packages `--use` names, loaded through `runtime`, each with its dependencies, as
    /// v1 loaded them.
    pub(super) fn used_packages<R>(
        &self,
        runtime: &Arc<MonitoringRuntime<R>>,
    ) -> Result<Vec<UsedPackage>, Error>
    where
        R: Runtime + Send + Sync + 'static,
    {
        let mut used = Vec::new();
        for name in &self.wasi.uses {
            let specifier = name
                .parse::<PackageSource>()
                .with_context(|| format!("Unable to parse \"{name}\" as a package specifier"))?;
            let rt = runtime.clone();
            let pkg = runtime
                .task_manager()
                .spawn_and_block_on(async move {
                    BinaryPackage::from_registry(&specifier, rt.as_ref()).await
                })
                .with_context(|| format!("Unable to load \"{name}\""))?
                .with_context(|| format!("Unable to load \"{name}\""))?;
            // The tree the runtime loaded last is this package's.
            let tree = runtime.package_tree();
            used.push(UsedPackage {
                volumes: self.package_volumes(&tree, None)?,
                commands: package_commands(&pkg)?,
            });
        }
        Ok(used)
    }

    /// The volumes of the tree's filesystem table, each after the ones it obscures and parents
    /// before what is mounted inside them. A package directory's own table maps host
    /// directories instead; they arrive as `--volume`s. `file` is the file the tree's root was
    /// loaded from, when it was not downloaded.
    fn package_volumes(
        &self,
        tree: &PackageTree,
        file: Option<&Path>,
    ) -> Result<Vec<PackageVolume>, Error> {
        let mut volumes = Vec::new();
        let mut readers: HashMap<&PackageId, Reader> = HashMap::new();
        // Mappings earlier in the table obscure later ones, so they are mounted last.
        for mapping in tree.filesystem.iter().rev() {
            let is_root = tree.root.as_ref() == Some(&mapping.package);
            if is_root && tree.root_is_local_dir {
                continue;
            }
            let mount_path = mapping.mount_path.to_str().with_context(|| {
                format!(
                    "the mount path {} is not UTF-8",
                    mapping.mount_path.display()
                )
            })?;
            if !mount_path.starts_with('/') {
                bail!(
                    "the package \"{}\" mounts a volume at a relative path, {mount_path}",
                    mapping.package
                );
            }
            let reader = match readers.entry(&mapping.package) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let container = tree.containers.get(&mapping.package).with_context(|| {
                        format!(
                            "the package \"{}\" is not in the dependency tree",
                            mapping.package
                        )
                    })?;
                    let file = match (file, is_root) {
                        (Some(path), true) => Some(path),
                        _ => tree.files.get(&mapping.package).map(PathBuf::as_path),
                    };
                    entry.insert(Reader::open(&mapping.package, container, file)?)
                }
            };
            // A v2 mapping names the "atom" volume and a path in it, which migration made a
            // volume of its own.
            let name = match (reader.migrated, &mapping.original_path) {
                (true, Some(path)) => path.clone(),
                _ => mapping.volume_name.clone(),
            };
            // Migration keeps only the parts of "atom" an `fs` table names, so a v2 package
            // without one (whose "atom" v1 mounted at `/`) has nothing to mount.
            if reader.migrated && reader.v3.get_volume(&name).is_err() {
                tracing::warn!(
                    package = %mapping.package,
                    volume = %name,
                    "the webc v2 package has no `fs` table; its \"atom\" volume is not mounted",
                );
                continue;
            }
            volumes.push(PackageVolume {
                reader: reader.v3.clone(),
                name,
                mount_path: mount_path.to_string(),
            });
        }
        // A stable sort keeps the obscuring order among mounts at the same depth.
        volumes.sort_by_key(|v| Path::new(&v.mount_path).components().count());
        Ok(volumes)
    }

    /// Runs `program` on a machine whose images compile with `loader`, driven on the tokio
    /// runtime `handle`, and exits with its status. A program started from a file runs as if
    /// it had run on `engine` before.
    pub(super) fn execute_machine(
        &self,
        program: MachineProgram,
        loader: Arc<CachedLoader>,
        engine: &Engine,
        handle: &tokio::runtime::Handle,
    ) -> Result<(), Error> {
        self.reject_options_unsupported_by_the_machine(program.is_package())?;

        let wasi = program.wasi.as_ref();
        let stdio = Stdio::inherit();
        // A terminal on standard input becomes the guest's console, unless `--no-tty`.
        #[cfg(unix)]
        let terminal = (!self.wasi.no_tty).then(HostTerminal::detect).flatten();
        #[cfg(unix)]
        let console = terminal
            .map(|terminal| terminal.console(&stdio))
            .transpose()
            .context("cannot use the terminal as the guest's console")?;
        #[cfg(not(unix))]
        let console = None;
        let mut machine = self.machine(loader, console)?;
        let config = ProcessConfig {
            args: std::iter::once(program.argv0.clone())
                .chain(wasi.and_then(|w| w.main_args.clone()).unwrap_or_default())
                .chain(self.args.iter().cloned())
                .collect(),
            env: self.machine_env(machine.default_env(), wasi)?,
            stdio,
        };
        let (root_volumes, volumes): (Vec<_>, Vec<_>) = program
            .volumes
            .iter()
            .partition(|v| Path::new(&v.mount_path) == Path::new("/"));
        let (have_current_dir, host_dirs) = self.wasi.build_mapped_directories()?;
        // Nothing can be mounted over the root, so a host directory for `/` joins its layers,
        // over the package's.
        let (root_dirs, host_dirs): (Vec<_>, Vec<_>) = host_dirs
            .into_iter()
            .partition(|dir| Path::new(&dir.guest) == Path::new("/"));
        let mounts = volumes.iter().map(|v| v.mount_path.clone());
        let hiding = program
            .mount_hiding_the_command(mounts.chain(host_dirs.iter().map(|d| d.guest.clone())));
        // Each volume becomes a filesystem once, wherever it is mounted.
        let mut layers = VolumeLayers::default();
        let mut root_layers = Vec::new();
        for volume in root_volumes {
            let layer = layers.get(&machine, volume).with_context(|| {
                format!("cannot mount the package volume \"{}\" at /", volume.name)
            })?;
            root_layers.push(layer);
        }
        // Over the package's volumes, as v1 wrote them into the root after merging those.
        let commands = program
            .commands
            .iter()
            .map(|cmd| (cmd.name.clone(), cmd.atom.clone()));
        root_layers.extend(
            machine
                .commands_layer(commands)
                .context("cannot install the package's commands in /bin and /usr/bin")?,
        );
        for dir in root_dirs {
            let layer = machine
                .host_dir_layer(&dir.host)
                .with_context(|| format!("cannot map {} to /", dir.host.display()))?;
            root_layers.push(layer);
        }
        handle
            .block_on(
                tokio::task::LocalSet::new().run_until(machine.mount_root_layers(root_layers)),
            )
            .context("cannot stack the root filesystem's layers")?;
        let defaults = program
            .commands
            .iter()
            .map(|cmd| (cmd.name.clone(), cmd.defaults.clone()));
        machine
            .set_wasix1_command_defaults(defaults)
            .context("cannot configure the package's commands")?;
        // v1's precedence: the command's `cwd` annotation, then `--cwd`, then the mapped current
        // directory.
        let cwd = wasi
            .and_then(|w| w.cwd.as_deref())
            .or(self.wasi.cwd.as_deref());
        if let Some(cwd) = cwd {
            if !cwd.starts_with("/") {
                bail!("The working directory must be an absolute path");
            }
            machine.set_working_dir(cwd.display().to_string());
        } else if have_current_dir {
            machine.set_working_dir(MAPPED_CURRENT_DIR_DEFAULT_PATH);
        }
        // The machine's host calls are not `Send`, so drive it on this thread, in a `LocalSet`
        // for the host work it spawns.
        let run = handle.block_on(tokio::task::LocalSet::new().run_until(async {
            for volume in volumes {
                let context = || {
                    format!(
                        "cannot mount the package volume \"{}\" at {}",
                        volume.name, volume.mount_path
                    )
                };
                let layer = layers.get(&machine, volume).with_context(context)?;
                // Writable like v1's package mounts; changes stay in memory.
                machine
                    .mount_layer(&volume.mount_path, &layer, true)
                    .await
                    .with_context(context)?;
            }
            // `--volume`s go on top, so they win over a package volume at the same path.
            for dir in &host_dirs {
                machine
                    .mount_host_dir(&dir.guest, &dir.host, false)
                    .await
                    .with_context(|| {
                        format!("cannot map {} to {}", dir.host.display(), dir.guest)
                    })?;
            }
            if let Some(dir) = &program.script_dir {
                machine
                    .mount_host_dir(SCRIPT_DIR, dir, true)
                    .await
                    .with_context(|| format!("cannot map {} to {SCRIPT_DIR}", dir.display()))?;
            }
            // Each publication lasts as long as its handle.
            let _publications = self
                .wasi
                .publish
                .iter()
                .map(|port| {
                    let guest = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port.guest_port);
                    machine
                        .network()
                        .publish(port.host, guest)
                        .with_context(|| {
                            format!(
                                "cannot publish {} to guest port {}",
                                port.host, port.guest_port
                            )
                        })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            // Installed before the guest starts, so no signal takes the host's default action.
            #[cfg(unix)]
            forward_signals(handle, machine.signaller(), terminal)
                .context("cannot install signal handlers")?;
            #[cfg(not(unix))]
            forward_signals(handle, machine.signaller())
                .context("cannot install signal handlers")?;
            // Restored when the run ends, before `wasmer` reports anything or exits.
            #[cfg(unix)]
            let _raw = terminal
                .map(|terminal| terminal.raw())
                .transpose()
                .context("cannot put the terminal in raw mode")?;
            let status = match &program.start {
                Start::Module { engine, module } => {
                    machine.run_module(engine, module, config).await
                }
                Start::File(path) => machine.run_file(engine, path, config).await,
            };
            Ok::<_, Error>(status)
        }));
        let result = match run {
            Ok(Ok(0)) => Ok(()),
            Ok(Ok(status)) => Err(GuestExit(status).into()),
            Ok(Err(err)) => Err(explain_hidden_command(err, &program, hiding)),
            Err(err) => Err(err),
        };
        // Exit before the tokio runtime is dropped: a guest blocked reading stdin leaves an
        // uncancellable read on the blocking pool, and dropping the runtime would wait for it.
        exit_with_wasi_exit_code(result)
    }

    /// The guest path of the host script at `path`: where the `--volume` mapping holding it
    /// (the innermost one, for nested mappings) shows it, or else in [`SCRIPT_DIR`], with its
    /// directory, which is returned too, to map there.
    pub(super) fn script_guest_path(
        &self,
        path: &Path,
    ) -> Result<(String, Option<PathBuf>), Error> {
        let script = path
            .canonicalize()
            .with_context(|| format!("Unable to find \"{}\"", path.display()))?;
        let (_, mapped) = self.wasi.build_mapped_directories()?;
        let in_volume = mapped
            .iter()
            .filter_map(|dir| Some((dir, script.strip_prefix(&dir.host).ok()?)))
            .max_by_key(|(dir, _)| dir.host.components().count());
        let (guest_dir, rest, script_dir) = match in_volume {
            Some((dir, rest)) => (dir.guest.as_str(), rest, None),
            None => {
                let dir = script
                    .parent()
                    .context("a script is a file in a directory")?;
                let name = script.strip_prefix(dir)?;
                (SCRIPT_DIR, name, Some(dir.to_path_buf()))
            }
        };
        let rest = rest
            .to_str()
            .with_context(|| format!("the path {} is not UTF-8", rest.display()))?;
        let mut guest = guest_dir.trim_end_matches('/').to_string();
        for part in rest.split(std::path::MAIN_SEPARATOR) {
            guest.push('/');
            guest.push_str(part);
        }
        Ok((guest, script_dir))
    }

    /// A machine configured by `--user`, `--no-init` and the `uname` options, whose images
    /// compile with `loader`, with `console` if given.
    fn machine(
        &self,
        loader: Arc<CachedLoader>,
        console: Option<(Console, [bool; 3])>,
    ) -> Result<Machine, Error> {
        let wasi = &self.wasi;
        let mut builder = Machine::builder().loader(loader).init(!wasi.no_init);
        if let Some((console, fds)) = console {
            builder = builder.console(console, fds);
        }
        if let Some(user) = wasi.user {
            builder = builder.user(Credentials::new(user.uid, user.gid));
        }
        let mut uname = Uname::default();
        if let Some(name) = &wasi.hostname {
            uname = uname.with_nodename(name).context("invalid --hostname")?;
        }
        if let Some(name) = &wasi.domainname {
            uname = uname
                .with_domainname(name)
                .context("invalid --domainname")?;
        }
        if let Some(release) = &wasi.uname_release {
            uname = uname
                .with_release(release)
                .context("invalid --uname-release")?;
        }
        if let Some(version) = &wasi.uname_version {
            uname = uname
                .with_version(version)
                .context("invalid --uname-version")?;
        }
        if let Some(machine) = &wasi.uname_machine {
            uname = uname
                .with_machine(machine)
                .context("invalid --uname-machine")?;
        }
        Ok(builder.uname(uname).build())
    }

    /// `defaults` (the machine's `HOME` and `PATH`), then v1's sources in v1's precedence:
    /// the command's `env` annotation, forwarded host variables, the env file, then `--env`;
    /// a later value for a name replaces an earlier one.
    fn machine_env(
        &self,
        defaults: Vec<String>,
        wasi: Option<&WasiAnnotation>,
    ) -> Result<Vec<String>, Error> {
        let mut env: Vec<(String, String)> = Vec::new();
        let mut set = |key: String, value: String| {
            env.retain(|(k, _)| *k != key);
            env.push((key, value));
        };
        let annotation = wasi.and_then(|w| w.env.as_deref()).unwrap_or_default();
        for item in defaults.iter().chain(annotation) {
            let (key, value) = item.split_once('=').unwrap_or((item, ""));
            set(key.to_string(), value.to_string());
        }
        if self.wasi.forward_host_env {
            // The kernel's environment is UTF-8, so a variable that is not stays behind.
            for (key, value) in std::env::vars_os() {
                match (utf8_env_part(key), utf8_env_part(value)) {
                    (Ok(key), Ok(value)) => set(key, value),
                    (Err(err), _) | (_, Err(err)) => {
                        tracing::warn!(%err, "not forwarding a host environment variable");
                    }
                }
            }
        }
        for (key, value) in self.wasi.resolved_env_vars()? {
            set(key, value);
        }
        Ok(env
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect())
    }

    fn reject_options_unsupported_by_the_machine(&self, is_package: bool) -> Result<(), Error> {
        let wasi = &self.wasi;
        #[allow(unused_mut)]
        let mut given = vec![
            ("--map-command", !wasi.map_commands.is_empty()),
            (
                "network rulesets (--net=<RULES>)",
                matches!(wasi.networking, Some(Some(_))),
            ),
            ("--http-client", wasi.http_client),
            (
                "--enable-async-threads",
                wasi.enable_async_threads.is_some(),
            ),
            ("--enable-cpu-backoff", wasi.enable_cpu_backoff.is_some()),
            // For a package it picks the command.
            ("--entrypoint", !is_package && self.entrypoint.is_some()),
            // Every program starts at `_start`.
            (
                "--invoke",
                self.invoke.as_deref().is_some_and(|f| f != "_start"),
            ),
            ("--experimental-napi", self.experimental_napi),
            // A trap kills the process with its signal; the kernel writes no core.
            ("--coredump-on-trap", self.coredump_on_trap.is_some()),
        ];
        #[cfg(feature = "journal")]
        given.extend([
            ("--journal", !wasi.read_only_journals.is_empty()),
            ("--journal-writable", !wasi.writable_journals.is_empty()),
            ("--snapshot-on", !wasi.snapshot_on.is_empty()),
            ("--snapshot-period", wasi.snapshot_interval.is_some()),
            ("--stop-after-snapshot", wasi.stop_after_snapshot),
        ]);

        let unsupported: Vec<_> = given
            .into_iter()
            .filter_map(|(name, is_given)| is_given.then_some(name))
            .collect();
        if !unsupported.is_empty() {
            bail!(
                "the WASIX v2 kernel does not support {} yet",
                unsupported.join(", ")
            );
        }
        Ok(())
    }
}

/// Says why the package's command could not be found when the mount `hiding` covers its
/// directory: the kernel only knows that the file is missing.
fn explain_hidden_command(
    err: wasixv2::RunError,
    program: &MachineProgram,
    hiding: Option<String>,
) -> Error {
    let not_found = matches!(
        &err,
        wasixv2::RunError::Execve { err, .. } if err.kind() == io::ErrorKind::NotFound
    );
    match (hiding, &program.start) {
        (Some(mount), Start::File(path)) if not_found => Error::from(err).context(format!(
            "the package's command {path} is hidden by the volume mounted at {mount}"
        )),
        _ => err.into(),
    }
}

/// Without a console, after how many `SIGINT`s (Ctrl-C) in a row that the guest leaves
/// unanswered `wasmer run` gives up on it and exits by itself. A press is unanswered when the next one finds it still pending,
/// or the kernel discarded it because every process it went to ignores `SIGINT`. A press that
/// finds the previous one taken starts the count again, so a guest that handles every Ctrl-C,
/// such as a shell or a REPL, is never cut short; one that ignores SIGINT, keeps it blocked, or
/// is stuck where it cannot take it, goes on the third.
///
/// Ctrl-C goes to the process group of the process `wasmer run` started, as standard input is
/// not the guest's terminal: a job a guest shell moved to a group of its own does not get it,
/// and does not count. On a console, `^C` is the guest's own: the terminal is raw, and the
/// console's line discipline sends SIGINT to its foreground process group.
const SIGINTS_TO_FORCE_EXIT: u32 = 3;

/// The guest ABI's `SIGINT` (Linux's numbering).
const GUEST_SIGINT: i32 = 2;

/// Counts the Ctrl-Cs in a row the guest left unanswered; see [`SIGINTS_TO_FORCE_EXIT`].
#[derive(Default)]
struct Interrupts {
    unanswered: u32,
    /// Whether the kernel left the last press pending, rather than discarding it.
    pending: bool,
}

impl Interrupts {
    /// A new Ctrl-C: whether it is the last one, so `wasmer run` exits instead of forwarding
    /// it.
    fn press(&mut self, signaller: &Signaller) -> bool {
        let previous_unanswered =
            self.unanswered > 0 && (!self.pending || signaller.foreground_pending(GUEST_SIGINT));
        self.unanswered = match previous_unanswered {
            true => self.unanswered + 1,
            false => 1,
        };
        self.unanswered >= SIGINTS_TO_FORCE_EXIT
    }

    /// Forwards the press to the guest.
    fn forward(&mut self, signaller: &Signaller) {
        self.pending = forward(signaller, GUEST_SIGINT, true);
    }
}

/// Forwards the signals a terminal or supervisor sends `wasmer run` to the guest, as signals from
/// outside the machine. `^C` and `^\` go to the foreground process group as a terminal sends
/// them, the others to the process `wasmer run` started, which is also its session leader, as
/// `kill` and a hangup would. A guest without a handler for a signal dies of it, and `wasmer run`
/// then exits with the guest's status, 128 + the signal.
///
/// With the host's `terminal` as the guest's console, the terminal is raw, so these come from
/// `kill` only: SIGINT and SIGQUIT go to the console's foreground group as its `^C` and `^\`
/// would, and SIGHUP hangs the console up, as the terminal went away. A new window size goes to
/// the console, and `wasmer` gives the terminal its settings back while it is stopped.
///
/// The handlers listen on the runtime's worker threads, so they work even when the thread driving
/// the guest is stuck; without a console, [`SIGINTS_TO_FORCE_EXIT`] is the way out then.
#[cfg(unix)]
fn forward_signals(
    runtime: &tokio::runtime::Handle,
    signaller: Signaller,
    terminal: Option<HostTerminal>,
) -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    // Host signal, its number in the guest's ABI, and whether a terminal sends it.
    const FORWARDED: [(i32, i32, bool); 6] = [
        (libc::SIGHUP, 1, false),
        (libc::SIGINT, GUEST_SIGINT, true),
        (libc::SIGQUIT, 3, true),
        (libc::SIGUSR1, 10, false),
        (libc::SIGUSR2, 12, false),
        (libc::SIGTERM, 15, false),
    ];

    let _context = runtime.enter();
    let console = terminal.is_some();
    for (host, guest, from_terminal) in FORWARDED {
        let mut stream = signal(SignalKind::from_raw(host))?;
        let signaller = signaller.clone();
        runtime.spawn(async move {
            let mut interrupts = Interrupts::default();
            while stream.recv().await.is_some() {
                if console && host == libc::SIGHUP {
                    signaller.console_hangup();
                    continue;
                }
                if console || guest != GUEST_SIGINT {
                    forward(&signaller, guest, from_terminal);
                    continue;
                }
                if interrupts.press(&signaller) {
                    std::process::exit(128 + GUEST_SIGINT);
                }
                interrupts.forward(&signaller);
            }
        });
    }
    if let Some(terminal) = terminal {
        forward_terminal_changes(runtime, signaller, terminal)?;
    }
    Ok(())
}

/// What happens to the host's `terminal` while the guest has it as its console: a new window
/// size goes to the console (SIGWINCH), and a stop of `wasmer` (SIGTSTP) gives the terminal
/// its settings back until `wasmer` is continued (SIGCONT), when it is raw again, at the size
/// it has then.
#[cfg(unix)]
fn forward_terminal_changes(
    runtime: &tokio::runtime::Handle,
    signaller: Signaller,
    terminal: HostTerminal,
) -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    for host in [libc::SIGWINCH, libc::SIGTSTP, libc::SIGCONT] {
        let mut stream = signal(SignalKind::from_raw(host))?;
        let signaller = signaller.clone();
        runtime.spawn(async move {
            while stream.recv().await.is_some() {
                if host == libc::SIGTSTP {
                    terminal::suspend();
                    // The stop SIGTSTP would have been, now that the terminal is restored. To
                    // this thread, which stops before it goes on to raw mode again: a signal
                    // to the process might be taken by another thread, later.
                    // SAFETY: a signal to the calling thread.
                    unsafe { libc::raise(libc::SIGSTOP) };
                }
                if host != libc::SIGWINCH {
                    terminal::resume();
                }
                signaller.console_resize(terminal.size());
            }
        });
    }
    Ok(())
}

/// Forwards Ctrl-C as `SIGINT`; see the Unix version. Exits with 3, as the C runtime does.
#[cfg(windows)]
fn forward_signals(runtime: &tokio::runtime::Handle, signaller: Signaller) -> io::Result<()> {
    let _context = runtime.enter();
    let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
    runtime.spawn(async move {
        let mut interrupts = Interrupts::default();
        while ctrl_c.recv().await.is_some() {
            if interrupts.press(&signaller) {
                std::process::exit(3);
            }
            interrupts.forward(&signaller);
        }
    });
    Ok(())
}

/// Sends `sig` to the guest; for a terminal's signal, returns whether some process has it
/// pending now.
fn forward(signaller: &Signaller, sig: i32, terminal: bool) -> bool {
    tracing::debug!(sig, "forwarding a host signal to the guest");
    let sent = match terminal {
        true => signaller.signal_foreground(sig),
        false => signaller.signal(sig).map(|()| false),
    };
    sent.unwrap_or_else(|err| {
        tracing::warn!(sig, %err, "cannot forward a signal to the guest");
        false
    })
}

/// The layer of each package volume mounted so far, by its package's reader and its name.
#[derive(Default)]
struct VolumeLayers(HashMap<(*const OwnedReader, String), Layer>);

impl VolumeLayers {
    /// The layer of `volume`, made the first time.
    fn get(&mut self, machine: &Machine, volume: &PackageVolume) -> Result<Layer, Error> {
        let key = (Arc::as_ptr(&volume.reader), volume.name.clone());
        if let Some(layer) = self.0.get(&key) {
            return Ok(layer.clone());
        }
        let layer = machine.package_layer(&volume.reader, &volume.name)?;
        self.0.insert(key, layer.clone());
        Ok(layer)
    }
}

/// A package's webc v3 reader, which the machine's package volumes need.
struct Reader {
    v3: Arc<OwnedReader>,
    /// Whether the package is webc v2, migrated in memory.
    migrated: bool,
}

impl Reader {
    /// `container`, the package `id`, which was loaded from `file` if known. The reader maps
    /// the file; without one, only a container holding a v3 reader works. A webc v2 package
    /// is migrated, which needs its bytes, so its file.
    ///
    /// The file is mapped and its index parsed again although the container holds a parsed
    /// reader: webc 12.0.1's `Container::downcast` tests the type of the `Container` itself,
    /// not of the reader inside it, so it never yields that reader (and the branch without a
    /// file always fails). Share the container's reader once webc fixes that.
    fn open(id: &PackageId, container: &Container, file: Option<&Path>) -> Result<Self, Error> {
        let v3 = match (container.version(), file) {
            (webc::Version::V3, Some(path)) => OwnedReader::from_path(path)
                .with_context(|| format!("Unable to read \"{}\"", path.display()))?,
            (webc::Version::V3, None) => {
                return container
                    .clone()
                    .downcast::<OwnedReader>()
                    .map(|v3| Self {
                        v3,
                        migrated: false,
                    })
                    .map_err(|_| anyhow::anyhow!("the webc v3 package \"{id}\" has no v3 reader"));
            }
            (webc::Version::V2, Some(path)) => {
                let bytes = std::fs::read(path)
                    .with_context(|| format!("Unable to read \"{}\"", path.display()))?;
                let migrated = webc::migration::v2_to_v3(bytes)
                    .with_context(|| format!("cannot migrate \"{}\" to webc v3", path.display()))?;
                let v3 = OwnedReader::parse(migrated).with_context(|| {
                    format!("the webc v3 migration of \"{}\" is invalid", path.display())
                })?;
                return Ok(Self {
                    v3: Arc::new(v3),
                    migrated: true,
                });
            }
            (webc::Version::V2, None) => bail!(
                "the package \"{id}\" is webc v2 and its file is unknown, so it cannot be \
                 migrated to the webc v3 the WASIX v2 kernel needs"
            ),
            (other, _) => bail!("the package \"{id}\" has the unsupported webc version {other}"),
        };
        Ok(Self {
            v3: Arc::new(v3),
            migrated: false,
        })
    }
}
