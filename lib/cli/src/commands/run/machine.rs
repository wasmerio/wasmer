//! Runs programs on the WASIX v2 kernel (the `wasixv2` crate's [`Machine`]): WASIX v2 modules,
//! WASIX v1 and WASI preview1 modules through the kernel's v1 compatibility ABI, and packages
//! whose command runs one of them.

use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error, bail};
use wasixv2::{Credentials, Machine, ProcessConfig, Signaller, Stdio, Uname};
use wasmer::{Engine, Module};
use wasmer_config::package::PackageId;
use wasmer_types::target::Target;
use wasmer_wasix::{
    Runtime,
    bin_factory::{BinaryPackage, BinaryPackageCommand},
    runners::MAPPED_CURRENT_DIR_DEFAULT_PATH,
    runtime::module_cache::HashedModuleData,
};
use webc::{
    Container,
    metadata::annotations::{WASI_RUNNER_URI, Wasi as WasiAnnotation},
    v3::read::OwnedReader,
};

use super::{
    CliPackageSource, GuestExit, Run, exit_with_wasi_exit_code, runtime::PackageTree,
    wasi::utf8_env_part,
};
use crate::backend::RuntimeOptions;

/// What to start on the machine.
pub(super) struct MachineProgram {
    argv0: String,
    engine: Engine,
    module: Module,
    /// The command's `wasi` annotation, for a package.
    wasi: Option<WasiAnnotation>,
    /// The package volumes to mount, each over the ones before it.
    volumes: Vec<PackageVolume>,
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
            engine,
            module,
            wasi: None,
            volumes: Vec::new(),
        }
    }

    fn is_package(&self) -> bool {
        self.wasi.is_some()
    }
}

/// Compiles `wasm` for the machine, through the runtime's compiled-module cache, with the
/// preemption points that let it deliver signals to a guest busy computing
/// ([`wasixv2::instrument`]).
pub(super) fn compile(
    runtime: &Arc<dyn Runtime + Send + Sync>,
    engine: &Engine,
    wasm: &[u8],
) -> Result<Module, Error> {
    let wasm = wasixv2::instrument(wasm).into_owned();
    Ok(runtime.load_hashed_module_sync(HashedModuleData::new(wasm), Some(engine))?)
}

/// Whether the machine runs `module`: one importing WASIX v2, or WASIX v1 or WASI preview1. A
/// module with neither is plain WebAssembly; a v1 flavour the kernel cannot serve is an error.
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

impl Run {
    /// The program to run for the package's selected command, chosen as `--entrypoint` or the
    /// package's entrypoint says. `tree` is the package tree `pkg` was loaded from.
    pub(super) fn machine_program(
        &self,
        pkg: &BinaryPackage,
        tree: &PackageTree,
        runtime: &Arc<dyn Runtime + Send + Sync>,
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
        let engine = self.engine_for_command(cmd)?;
        let module = compile(runtime, &engine, cmd.atom_ref())
            .with_context(|| format!("Unable to compile the \"{name}\" command"))?;
        if !runs_on_machine(&module)? {
            bail!("the \"{name}\" command's module imports neither WASIX nor WASI");
        }
        let wasi = cmd
            .metadata()
            .annotation::<WasiAnnotation>(WasiAnnotation::KEY)?
            .unwrap_or_else(|| WasiAnnotation::new(name));
        Ok(MachineProgram {
            argv0: wasi.exec_name.clone().unwrap_or_else(|| name.to_string()),
            engine,
            module,
            wasi: Some(wasi),
            volumes: self.package_volumes(tree)?,
        })
    }

    /// The engine for `cmd`: one whose backend has the WebAssembly features the command's atom
    /// declares, else the one the options pick.
    fn engine_for_command(&self, cmd: &BinaryPackageCommand) -> Result<Engine, Error> {
        let target = Target::default();
        if let Some(features) = cmd.wasm_features() {
            let backends = self.rt.get_available_backends()?;
            let compatible =
                RuntimeOptions::filter_backends_by_features(backends, &features, &target);
            if let Some(backend) = compatible.first() {
                tracing::info!(
                    command = cmd.name(),
                    ?features,
                    %backend,
                    "Picked a backend with the features the command needs",
                );
                return backend.get_engine(&target, &self.rt);
            }
        }
        self.rt.get_engine(&target)
    }

    /// The volumes of the tree's filesystem table, each after the ones it obscures and parents
    /// before what is mounted inside them. A package directory's own table maps host
    /// directories instead; they arrive as `--volume`s.
    fn package_volumes(&self, tree: &PackageTree) -> Result<Vec<PackageVolume>, Error> {
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
                    let file = match (&self.input, is_root) {
                        (CliPackageSource::File(path), true) => Some(path.as_path()),
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

    pub(super) fn execute_machine(
        &self,
        program: MachineProgram,
        runtime: &tokio::runtime::Handle,
    ) -> Result<(), Error> {
        self.reject_options_unsupported_by_the_machine(program.is_package())?;

        let wasi = program.wasi.as_ref();
        let config = ProcessConfig {
            args: std::iter::once(program.argv0.clone())
                .chain(wasi.and_then(|w| w.main_args.clone()).unwrap_or_default())
                .chain(self.args.iter().cloned())
                .collect(),
            env: self.machine_env(wasi)?,
            stdio: Stdio::inherit(),
        };
        let mut machine = self.machine()?;
        let (root_volumes, volumes): (Vec<_>, Vec<_>) = program
            .volumes
            .iter()
            .partition(|v| Path::new(&v.mount_path) == Path::new("/"));
        for volume in root_volumes {
            machine
                .mount_root_lower(&volume.reader, &volume.name)
                .with_context(|| {
                    format!("cannot mount the package volume \"{}\" at /", volume.name)
                })?;
        }
        let (have_current_dir, host_dirs) = self.wasi.build_mapped_directories()?;
        // Nothing can be mounted over the root, so a host directory for `/` joins its layers,
        // over the package's.
        let (root_dirs, host_dirs): (Vec<_>, Vec<_>) = host_dirs
            .into_iter()
            .partition(|dir| Path::new(&dir.guest) == Path::new("/"));
        for dir in root_dirs {
            machine
                .mount_root_lower_host_dir(&dir.host)
                .with_context(|| format!("cannot map {} to /", dir.host.display()))?;
        }
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
        let run = runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for volume in volumes {
                // Writable like v1's package mounts; changes stay in memory.
                machine
                    .mount_package_volume(&volume.mount_path, &volume.reader, &volume.name, true)
                    .await
                    .with_context(|| {
                        format!(
                            "cannot mount the package volume \"{}\" at {}",
                            volume.name, volume.mount_path
                        )
                    })?;
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
            forward_signals(runtime, machine.signaller())
                .context("cannot install signal handlers")?;
            Ok::<_, Error>(
                machine
                    .run_module(&program.engine, &program.module, config)
                    .await,
            )
        }));
        let result = match run {
            Ok(Ok(0)) => Ok(()),
            Ok(Ok(status)) => Err(GuestExit(status).into()),
            Ok(Err(err)) => Err(err.into()),
            Err(err) => Err(err),
        };
        // Exit before the tokio runtime is dropped: a guest blocked reading stdin leaves an
        // uncancellable read on the blocking pool, and dropping the runtime would wait for it.
        exit_with_wasi_exit_code(result)
    }

    /// A machine configured by `--user` and the `uname` options.
    fn machine(&self) -> Result<Machine, Error> {
        let wasi = &self.wasi;
        let mut builder = Machine::builder();
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

    /// Same sources and precedence as v1: the command's `env` annotation, then forwarded host
    /// variables, then the env file, then `--env`; a later value for a name replaces an
    /// earlier one.
    fn machine_env(&self, wasi: Option<&WasiAnnotation>) -> Result<Vec<String>, Error> {
        let mut env: Vec<(String, String)> = Vec::new();
        let mut set = |key: String, value: String| {
            env.retain(|(k, _)| *k != key);
            env.push((key, value));
        };
        for item in wasi.and_then(|w| w.env.as_deref()).unwrap_or_default() {
            let (key, value) = item.split_once('=').unwrap_or((item, ""));
            set(key.to_string(), value.to_string());
        }
        if self.wasi.forward_host_env {
            for (key, value) in std::env::vars_os() {
                set(utf8_env_part(key)?, utf8_env_part(value)?);
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
            ("--use", !wasi.uses.is_empty()),
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

/// After how many `SIGINT`s (Ctrl-C) in a row that the guest leaves pending `wasmer run` gives up
/// on it and exits by itself. A press that finds the previous one taken starts the count again,
/// so a guest that handles every Ctrl-C, such as a shell or a REPL, is never cut short; one that
/// keeps SIGINT blocked, or is stuck where it cannot take it, goes on the third.
const SIGINTS_TO_FORCE_EXIT: u32 = 3;

/// The guest ABI's `SIGINT` (Linux's numbering).
const GUEST_SIGINT: i32 = 2;

/// Forwards the signals a terminal or supervisor sends `wasmer run` to the guest, as signals from
/// outside the machine. `^C` and `^\` go to the foreground process group as a terminal sends
/// them, the others to the process `wasmer run` started, which is also its session leader, as
/// `kill` and a hangup would. A guest without a handler for a signal dies of it, and `wasmer run`
/// then exits with the guest's status, 128 + the signal.
///
/// The handlers listen on the runtime's worker threads, so they work even when the thread driving
/// the guest is stuck; [`SIGINTS_TO_FORCE_EXIT`] is the way out then.
#[cfg(unix)]
fn forward_signals(runtime: &tokio::runtime::Handle, signaller: Signaller) -> io::Result<()> {
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
    for (host, guest, terminal) in FORWARDED {
        let mut stream = signal(SignalKind::from_raw(host))?;
        let signaller = signaller.clone();
        runtime.spawn(async move {
            let mut unanswered = 0;
            while stream.recv().await.is_some() {
                if guest == GUEST_SIGINT {
                    unanswered = match signaller.foreground_pending(guest) {
                        true => unanswered + 1,
                        false => 1,
                    };
                    if unanswered >= SIGINTS_TO_FORCE_EXIT {
                        std::process::exit(128 + GUEST_SIGINT);
                    }
                }
                forward(&signaller, guest, terminal);
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
        let mut unanswered = 0;
        while ctrl_c.recv().await.is_some() {
            unanswered = match signaller.foreground_pending(GUEST_SIGINT) {
                true => unanswered + 1,
                false => 1,
            };
            if unanswered >= SIGINTS_TO_FORCE_EXIT {
                std::process::exit(3);
            }
            forward(&signaller, GUEST_SIGINT, true);
        }
    });
    Ok(())
}

fn forward(signaller: &Signaller, sig: i32, terminal: bool) {
    tracing::debug!(sig, "forwarding a host signal to the guest");
    let sent = match terminal {
        true => signaller.signal_foreground(sig),
        false => signaller.signal(sig),
    };
    if let Err(err) = sent {
        tracing::warn!(sig, %err, "cannot forward a signal to the guest");
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
