//! Runs modules that import `wasixv2`, and packages whose command runs one, on the experimental
//! WASIX v2 stack.

use std::{
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Error, bail};
use wasixv2::{Credentials, Machine, ProcessConfig, Signaller, Stdio, Uname};
use wasmer_wasix::{
    Runtime, WasiError, bin_factory::BinaryPackage, runners::MAPPED_CURRENT_DIR_DEFAULT_PATH,
    runtime::task_manager::VirtualTaskManagerExt as _,
};
use webc::{
    metadata::annotations::{FileSystemMapping, Wasi as WasiAnnotation},
    v3::read::OwnedReader,
};

use super::{CliPackageSource, Run, exit_with_wasi_exit_code, wasi::utf8_env_part};
use crate::backend::RuntimeOptions;

/// What to start on the WASIX v2 stack.
pub(super) struct Wasixv2Program {
    argv0: String,
    wasm: Vec<u8>,
    /// The command's `wasi` annotation, for a package.
    wasi: Option<WasiAnnotation>,
    /// The package whose volumes are mounted as its manifest's `fs` annotation says. `None` for a
    /// module, and for a package directory, whose `fs` table maps host directories instead
    /// (they arrive as `--volume`s, as for v1).
    package: Option<Arc<OwnedReader>>,
}

impl Wasixv2Program {
    pub(super) fn module(path: &Path, wasm: Vec<u8>) -> Self {
        Self {
            argv0: path.display().to_string(),
            wasm,
            wasi: None,
            package: None,
        }
    }

    fn is_package(&self) -> bool {
        self.wasi.is_some()
    }
}

impl Run {
    /// The program to run on WASIX v2 when the package's selected command is a WASIX v2
    /// module, or `None` to run the package on v1. The command is chosen as for v1.
    pub(super) fn wasixv2_package_program(
        &self,
        pkg: &BinaryPackage,
        runtime: &Arc<dyn Runtime + Send + Sync>,
    ) -> Result<Option<Wasixv2Program>, Error> {
        let name = match self.entrypoint.as_deref() {
            Some(name) => name,
            None => pkg.infer_entrypoint()?,
        };
        // A missing command is reported by the v1 path.
        let Some(cmd) = pkg.get_command(name) else {
            return Ok(None);
        };
        if !::wasixv2::is_wasixv2_module(cmd.atom_ref()) {
            return Ok(None);
        }
        let wasi = cmd
            .metadata()
            .annotation::<WasiAnnotation>(WasiAnnotation::KEY)?
            .unwrap_or_else(|| WasiAnnotation::new(name));
        let package = match &self.input {
            CliPackageSource::File(path) => Some(Arc::new(
                OwnedReader::from_path(path).with_context(|| {
                    format!(
                        "WASIX v2 needs a webc v3 package, and \"{}\" is not one",
                        path.display()
                    )
                })?,
            )),
            CliPackageSource::Dir(_) => None,
            CliPackageSource::Package(specifier) => {
                Some(load_registry_package(specifier, runtime)?)
            }
        };
        Ok(Some(Wasixv2Program {
            argv0: wasi.exec_name.clone().unwrap_or_else(|| name.to_string()),
            wasm: cmd.atom_ref().to_vec(),
            wasi: Some(wasi),
            package,
        }))
    }

    pub(super) fn execute_wasixv2(
        &self,
        program: Wasixv2Program,
        runtime: &tokio::runtime::Handle,
    ) -> Result<(), Error> {
        self.reject_options_unsupported_by_wasixv2(program.is_package())?;

        let wasi = program.wasi.as_ref();
        let config = ProcessConfig {
            args: std::iter::once(program.argv0.clone())
                .chain(wasi.and_then(|w| w.main_args.clone()).unwrap_or_default())
                .chain(self.args.iter().cloned())
                .collect(),
            env: self.wasixv2_env(wasi)?,
            stdio: Stdio::inherit(),
        };
        let mut machine = self.wasixv2_machine()?;
        let (have_current_dir, volumes) = self.wasi.build_mapped_directories(true)?;
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
        let package_mounts = match &program.package {
            Some(reader) => package_mounts(reader)?,
            None => Vec::new(),
        };
        // The machine's host calls are not `Send`, so drive it on this thread, in a `LocalSet`
        // for the host work it spawns.
        let run = runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for mapping in &package_mounts {
                let reader = program
                    .package
                    .as_deref()
                    .expect("mounts come from a package");
                // Writable like v1's package mounts; changes stay in memory.
                machine
                    .mount_package_volume(&mapping.mount_path, reader, &mapping.volume_name, true)
                    .await
                    .with_context(|| {
                        format!(
                            "cannot mount the package volume \"{}\" at {}",
                            mapping.volume_name, mapping.mount_path
                        )
                    })?;
            }
            // `--volume`s go on top, so they win over a package volume at the same path.
            for volume in &volumes {
                machine
                    .mount_host_dir(&volume.guest, &volume.host, false)
                    .await
                    .with_context(|| {
                        format!("cannot map {} to {}", volume.host.display(), volume.guest)
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
            Ok::<_, Error>(machine.run_process(&program.wasm, config).await?)
        }));
        let result = match run {
            Ok(0) => Ok(()),
            Ok(status) => Err(WasiError::Exit(status.into()).into()),
            Err(err) => Err(err),
        };
        // Exit before the tokio runtime is dropped: a guest blocked reading stdin leaves an
        // uncancellable read on the blocking pool, and dropping the runtime would wait for it.
        exit_with_wasi_exit_code(result)
    }

    /// A machine configured by `--user` and the `uname` options.
    fn wasixv2_machine(&self) -> Result<Machine, Error> {
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

    /// Same sources and precedence as the v1 runner: the command's `env` annotation, then
    /// forwarded host variables, then the env file, then `--env`; a later value for a name
    /// replaces an earlier one.
    fn wasixv2_env(&self, wasi: Option<&WasiAnnotation>) -> Result<Vec<String>, Error> {
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

    fn reject_options_unsupported_by_wasixv2(&self, is_package: bool) -> Result<(), Error> {
        let wasi = &self.wasi;
        #[allow(unused_mut)]
        let mut given = vec![
            ("--use", !wasi.uses.is_empty()),
            ("--include-webc", !wasi.include_webcs.is_empty()),
            ("--map-command", !wasi.map_commands.is_empty()),
            ("--net", wasi.networking.is_some()),
            ("--http-client", wasi.http_client),
            (
                "--enable-async-threads",
                wasi.enable_async_threads.is_some(),
            ),
            ("--enable-cpu-backoff", wasi.enable_cpu_backoff.is_some()),
            ("--stack-size", self.stack_size.is_some()),
            // For a package it picks the command.
            ("--entrypoint", !is_package && self.entrypoint.is_some()),
            ("--invoke", self.invoke.is_some()),
            ("--coredump-on-trap", self.coredump_on_trap.is_some()),
            ("--experimental-napi", self.experimental_napi),
            (
                "compiler and backend options",
                self.rt != RuntimeOptions::default(),
            ),
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
                "this module targets WASIX v2, which does not support {} yet",
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

/// The registry package's container, through the same source and loader (and so the same
/// download cache, memory-mapped) that resolved it for v1.
fn load_registry_package(
    specifier: &wasmer_config::package::PackageSource,
    runtime: &Arc<dyn Runtime + Send + Sync>,
) -> Result<Arc<OwnedReader>, Error> {
    let container = {
        let specifier = specifier.clone();
        let runtime = runtime.clone();
        runtime
            .task_manager()
            .clone()
            .spawn_and_block_on(async move {
                let summary = runtime.source().latest(&specifier).await?;
                runtime.package_loader().load(&summary).await
            })??
    };
    container.downcast::<OwnedReader>().map_err(|_| {
        anyhow::anyhow!("WASIX v2 needs a webc v3 package, and {specifier} is not one")
    })
}

/// The volumes of the package's `fs` annotation, parents before what is mounted inside them.
/// v1 mounts a webc v3 volume's root wherever the mapping says, whatever its `host_path`; so
/// does this. Volumes of dependencies and a volume at `/` are not supported yet.
fn package_mounts(reader: &OwnedReader) -> Result<Vec<FileSystemMapping>, Error> {
    let mut mappings = match reader.manifest().filesystem()? {
        Some(fs) => fs.0,
        None => return Ok(Vec::new()),
    };
    for mapping in &mappings {
        if let Some(dependency) = &mapping.from {
            bail!(
                "WASIX v2 cannot mount volumes of dependencies yet (\"{}\" from \"{dependency}\")",
                mapping.volume_name
            );
        }
        if !mapping.mount_path.starts_with('/') {
            bail!(
                "the package mounts a volume at a relative path, {}",
                mapping.mount_path
            );
        }
        if Path::new(&mapping.mount_path) == Path::new("/") {
            bail!(
                "WASIX v2 cannot mount a package volume (\"{}\") at / yet",
                mapping.volume_name
            );
        }
    }
    mappings.sort_by_key(|m| Path::new(&m.mount_path).components().count());
    Ok(mappings)
}
