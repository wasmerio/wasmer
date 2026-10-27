//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::{
    collections::HashSet,
    future::Future,
    io,
    path::Path,
    pin::pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

pub use shared_buffer::OwnedBuffer;
use wasixv2_exec::{Abis, Wasixv2};
pub use wasixv2_exec::{
    Compile, ExecError, Image, ImageId, LoadError, Loader, instrument, is_wasixv2_module, load,
};
#[cfg(not(target_family = "wasm"))]
pub use wasixv2_host::BlockingSource;
#[cfg(feature = "tokio")]
pub use wasixv2_host::TokioHost;
#[cfg(feature = "web")]
pub use wasixv2_host::WebHost;
pub use wasixv2_host::{
    ByteSink, ByteSource, EmptySource, Executor, FnSink, Host, LocalFuture, ThreadStart,
};
pub use wasixv2_kernel::{Credentials, ProcessConfig, Stdio, Uname, UnameError};
use wasixv2_kernel::{HostClock, Kernel};
use wasixv2_syscalls::{Layered, TaskId};
use wasixv2_trace::Trace;
pub use wasixv2_vfs::Filesystem;
use wasixv2_vfs::{Cred, Ctx, Errno, LookupFlags, MountFlags, NAME_MAX, Vfs};
use wasixv2_vfs_mem::MemFs;
use wasixv2_vfs_overlay::{MAX_LOWERS, OverlayFs};
use wasixv2_vfs_package::PackageFs;
pub use wasixv2_vnet::{Net, NetConfig};
#[cfg(feature = "wasix1")]
pub use wasixv2_wasix1::{
    CommandDefaults, V1Flavor, is_wasix1_module, module_flavor as wasix1_module_flavor,
};
pub use wasmer::Engine;
use wasmer::{Module, Store};
use webc::v3::read::OwnedReader;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("invalid module: {0}")]
    Compile(#[from] wasmer::CompileError),
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error("cannot use the working directory: {0}")]
    WorkingDir(io::Error),
    #[error("cannot execute {path}: {err}")]
    Execve { path: String, err: io::Error },
}

/// The directories [`Machine::install_commands`] puts commands in, relative to the root.
const COMMAND_DIRS: [&str; 2] = ["bin", "usr/bin"];

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and laid out as a small Linux system (see
/// [`default_root`]); [`MachineBuilder::root`] takes another, and [`Machine::mount_root_lower`]
/// stacks read-only layers, such as package volumes, under it. Either way the kernel's own
/// filesystems are mounted on it: `/proc`, `/dev` with the device nodes, and memory
/// filesystems at `/dev/shm` and `/tmp`. The network is one namespace; with the `host-network`
/// feature on a Unix host, traffic leaving it goes through host sockets, otherwise it is
/// loopback-only. The machine's futures must be driven where its [`Host`]'s executor expects
/// them; see [`Machine::new`].
///
/// Processes run as the machine's user ([`MachineBuilder::user`]), which also owns the root
/// filesystem, mapped host directories and package volumes.
pub struct Machine {
    syscalls: Arc<Layered<Trace, Kernel>>,
    /// The guest ABIs programs may target: `wasixv2`, then WASIX v1 with the `wasix1` feature.
    abis: Arc<Abis<Layered<Trace, Kernel>>>,
    /// The package commands WASIX v1 programs start with their defaults.
    #[cfg(feature = "wasix1")]
    wasix1: Wasix1Commands,
    network: Arc<Net>,
    host: Arc<dyn Host>,
    /// The uid and gid processes start as and filesystems are owned by.
    owner: (u32, u32),
    /// Where new processes start; `/` if unset.
    working_dir: Option<String>,
    /// The processes [`Machine::run`], [`Machine::run_process`], [`Machine::run_module`] and
    /// [`Machine::run_file`] started that are still running.
    running: Arc<Mutex<Vec<TaskId>>>,
    /// Who the processes start as and what `uname` reports, for a kernel booted on a new root.
    user: Credentials,
    uname: Uname,
    /// The root filesystem's writable top layer: the [`default_root`] or the embedder's.
    root_upper: Arc<dyn Filesystem>,
    /// Read-only layers under `root_upper`, topmost first; see [`Machine::mount_root_lower`].
    root_lowers: Vec<Arc<dyn Filesystem>>,
    /// Set by the first mount, process or [`Signaller`]: from then on the root filesystem is
    /// fixed.
    in_use: AtomicBool,
}

#[cfg(feature = "tokio")]
impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

/// Configures a [`Machine`] before it boots: who its processes run as, what `uname` reports
/// and the root filesystem. The defaults are those of [`Credentials`] and [`Uname`], and the
/// [`default_root`].
///
/// ```no_run
/// # use wasixv2::{Credentials, Machine, Uname};
/// let machine = Machine::builder()
///     .user(Credentials::new(0, 0))
///     .uname(Uname::default().with_release("6.1.0")?)
///     .build();
/// # Ok::<(), wasixv2::UnameError>(())
/// ```
pub struct MachineBuilder {
    host: Arc<dyn Host>,
    user: Credentials,
    uname: Uname,
    root: Option<Arc<dyn Filesystem>>,
    loader: Arc<dyn Loader>,
}

impl MachineBuilder {
    /// A machine whose clocks, timers, entropy and executor come from `host`.
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            user: Credentials::default(),
            uname: Uname::default(),
            root: None,
            loader: Arc::new(Compile),
        }
    }

    /// The identity the machine's processes start as, and that owns its filesystems. uid 0 is
    /// root, with every capability.
    pub fn user(mut self, user: Credentials) -> Self {
        self.user = user;
        self
    }

    /// The names `uname` reports.
    pub fn uname(mut self, uname: Uname) -> Self {
        self.uname = uname;
        self
    }

    /// Boots on `root` instead of the [`default_root`], such as an embedder's image. `/proc`,
    /// `/dev`, `/dev/shm` and `/tmp` are mounted on it, their directories made where missing;
    /// a mount point that cannot be made (on a read-only root) is left out. [`build`] mounts
    /// them at once, so `root` must answer at once, as memory filesystems do; layers that answer
    /// asynchronously, such as host directories, go under it with
    /// [`Machine::mount_root_lower_host_dir`].
    ///
    /// [`build`]: MachineBuilder::build
    pub fn root(mut self, root: Arc<dyn Filesystem>) -> Self {
        self.root = Some(root);
        self
    }

    /// What compiles the images processes `execve`, and the program [`Machine::run_file`]
    /// starts: by default [`Compile`], with the engine the process ran on and no cache. An
    /// embedder's loader picks the engine per image and caches compiled modules, telling images
    /// apart by [`Image::identity`] or their bytes. With the machine's ABIs it is the machine's
    /// set of binary formats: what its [`Loader::probe`] refuses, or no ABI serves, fails
    /// `execve` with `ENOEXEC`.
    pub fn loader(mut self, loader: Arc<dyn Loader>) -> Self {
        self.loader = loader;
        self
    }

    pub fn build(self) -> Machine {
        let Self {
            host,
            user,
            uname,
            root,
            loader,
        } = self;
        let owner = (user.uid(), user.gid());
        let network = default_network();
        let root = root.unwrap_or_else(|| default_root(&host, &network.resolv_conf(), &user));
        #[cfg(feature = "wasix1")]
        let wasix1 = Wasix1Commands::default();
        let abis = Arc::new(abis(
            loader,
            #[cfg(feature = "wasix1")]
            &wasix1,
        ));
        let kernel = boot(&host, root.clone(), &network, &user, &uname, abis.clone());
        now(mount_kernel_filesystems(&kernel, &host));
        Machine {
            syscalls: Arc::new(Layered::new(Trace, kernel)),
            abis,
            #[cfg(feature = "wasix1")]
            wasix1,
            network,
            host,
            owner,
            working_dir: None,
            running: Arc::default(),
            user,
            uname,
            root_upper: root,
            root_lowers: Vec::new(),
            in_use: AtomicBool::new(false),
        }
    }
}

impl Machine {
    /// A machine on [`TokioHost`] with the default configuration. Drive it on a tokio runtime
    /// with timers enabled, inside a `tokio::task::LocalSet`, which background host work is
    /// spawned onto.
    #[cfg(feature = "tokio")]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Configures a machine on [`TokioHost`]; see [`Machine::new`].
    #[cfg(feature = "tokio")]
    pub fn builder() -> MachineBuilder {
        MachineBuilder::new(Arc::new(TokioHost::default()))
    }

    /// A machine with the default configuration whose clocks, timers, entropy and executor
    /// come from `host`.
    pub fn with_host(host: Arc<dyn Host>) -> Self {
        MachineBuilder::new(host).build()
    }

    /// A machine on `root` instead of the default root filesystem; see
    /// [`MachineBuilder::root`].
    pub fn with_root(host: Arc<dyn Host>, root: Arc<dyn Filesystem>) -> Self {
        MachineBuilder::new(host).root(root).build()
    }

    /// Makes the volume `volume` of the webc v3 package `reader` a read-only layer of the root
    /// filesystem, under its top layer (the [`default_root`] or the embedder's root), which holds
    /// `/etc` and every change, and over the layers earlier calls added. Objects in the volume
    /// appear owned by the machine's user. This is how a package's volume at `/` is mounted. The
    /// kernel's filesystems are mounted on the new root as on the old one.
    ///
    /// The root can only change on a fresh machine: this fails with `EBUSY` after anything was
    /// mounted or run, or a [`Signaller`] was handed out.
    pub async fn mount_root_lower(&mut self, reader: &OwnedReader, volume: &str) -> io::Result<()> {
        let (uid, gid) = self.owner;
        let lower = PackageFs::with_owner(reader, volume, uid, gid).map_err(io::Error::other)?;
        self.push_root_lower(Arc::new(lower)).await
    }

    /// [`Machine::mount_root_lower`] for the host directory `host_dir`, which the guest sees
    /// through the root but never changes: changes stay in the top layer. This is how a
    /// host directory mapped to `/` is mounted, since nothing can be mounted over the root.
    pub async fn mount_root_lower_host_dir(&mut self, host_dir: &Path) -> io::Result<()> {
        let options = wasixv2_vfs_host::Options {
            read_only: true,
            owner: self.owner,
        };
        let lower = wasixv2_vfs_host::open(host_dir, self.host.clone(), options)?;
        self.push_root_lower(lower).await
    }

    /// Makes each of `commands`, a name and the program's bytes, an executable file at
    /// `/bin/<name>` and `/usr/bin/<name>`, as a package tree's commands are. The files are a
    /// read-only layer of the root filesystem over the ones earlier calls added (package
    /// volumes), so `/bin` and `/usr/bin` keep what those have there and a command hides a
    /// file of its name; the guest can still add, replace and remove entries, and the changes
    /// stay in memory. Every file is a regular `0755` file owned by the machine's user, whose
    /// contents are the bytes, shared with the buffer; `execve` loads it like any other file.
    /// Of several commands with one name, the first wins.
    ///
    /// Fails with `EINVAL` for a name that cannot be a file name (empty, `.`, `..`, or with
    /// `/` or NUL), `ENAMETOOLONG` for one longer than `NAME_MAX`, and, like
    /// [`Machine::mount_root_lower`], with `EBUSY` after anything was mounted or run.
    pub async fn install_commands(
        &mut self,
        commands: impl IntoIterator<Item = (String, OwnedBuffer)>,
    ) -> io::Result<()> {
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for (name, image) in commands {
            if matches!(name.as_str(), "" | "." | "..") || name.contains(['/', '\0']) {
                return Err(errno_to_io(Errno::EINVAL));
            }
            if name.len() > NAME_MAX {
                return Err(errno_to_io(Errno::ENAMETOOLONG));
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            for dir in COMMAND_DIRS {
                files.push((format!("{dir}/{name}"), image.clone()));
            }
        }
        let (uid, gid) = self.owner;
        let layer = PackageFs::from_files(files, uid, gid).map_err(io::Error::other)?;
        self.push_root_lower(Arc::new(layer)).await
    }

    /// Gives WASIX v1 programs v1's package command semantics: a v1 program that starts the
    /// file `/bin/<name>` or `/usr/bin/<name>` of a command in `commands`, by any name for it
    /// (a relative path, a symlink), passes it the command's [`CommandDefaults`] (its `wasi`
    /// annotation's `exec-name`, `main-args` and `env`), as v1 did. The files are those there
    /// when the first process starts. Programs of other ABIs start commands as Linux starts any
    /// file. Replaces what an earlier call gave; fails with `EBUSY` once anything ran.
    #[cfg(feature = "wasix1")]
    pub fn set_wasix1_command_defaults(
        &mut self,
        commands: impl IntoIterator<Item = (String, CommandDefaults)>,
    ) -> io::Result<()> {
        if *self.in_use.get_mut() {
            return Err(errno_to_io(Errno::EBUSY));
        }
        *self.wasix1.names.lock().unwrap() = commands.into_iter().collect();
        Ok(())
    }

    /// Fills the table of v1 package commands from the names registered, by the files they
    /// name now. Once, at the first process: the root no longer changes then.
    #[cfg(feature = "wasix1")]
    async fn resolve_wasix1_commands(&self) {
        let names = std::mem::take(&mut *self.wasix1.names.lock().unwrap());
        if names.is_empty() {
            return;
        }
        let vfs = self.vfs();
        let cred = Cred::root();
        let top = vfs.root();
        let cx = Ctx {
            cred: &cred,
            root: &top,
            umask: 0,
            task: 0,
        };
        let mut table = wasixv2_wasix1::Commands::default();
        for (name, defaults) in names {
            for dir in COMMAND_DIRS {
                let path = format!("/{dir}/{name}");
                let file = match vfs
                    .lookup(&cx, &top, path.as_bytes(), LookupFlags::follow())
                    .await
                {
                    Ok(file) => file,
                    // Hidden by a mount, or removed: nothing to start there.
                    Err(_) => continue,
                };
                let Ok(attr) = vfs.stat(&file).await else {
                    continue;
                };
                let dev = attr.dev.unwrap_or_default();
                let id = wasixv2_wasix1::FileId {
                    dev: (dev.major, dev.minor),
                    ino: attr.ino,
                };
                table.insert(id, defaults.clone());
            }
        }
        *self.wasix1.files.write().unwrap() = table;
    }

    async fn push_root_lower(&mut self, lower: Arc<dyn Filesystem>) -> io::Result<()> {
        if *self.in_use.get_mut() {
            return Err(errno_to_io(Errno::EBUSY));
        }
        if self.root_lowers.len() == MAX_LOWERS {
            return Err(io::Error::other("too many layers stacked under the root"));
        }
        self.root_lowers.insert(0, lower);
        let root = OverlayFs::new(Some(self.root_upper.clone()), self.root_lowers.clone());
        // Nothing lives in the kernel yet, so a new one on the new root replaces it, with the
        // kernel's filesystems mounted afresh; a host directory's layer answers asynchronously.
        let kernel = boot(
            &self.host,
            root,
            &self.network,
            &self.user,
            &self.uname,
            self.abis.clone(),
        );
        mount_kernel_filesystems(&kernel, &self.host).await;
        self.syscalls = Arc::new(Layered::new(Trace, kernel));
        Ok(())
    }

    /// The machine's network namespace, e.g. to [`Net::publish`] a host port to a guest
    /// listener.
    pub fn network(&self) -> &Arc<Net> {
        &self.network
    }

    fn vfs(&self) -> &Arc<Vfs> {
        self.syscalls.inner.vfs()
    }

    /// Mounts `fs` at `guest` (an absolute guest path), creating missing directories on the
    /// way, over whatever is there.
    pub async fn mount(
        &self,
        guest: &str,
        fs: Arc<dyn Filesystem>,
        read_only: bool,
    ) -> io::Result<()> {
        let flags = MountFlags {
            read_only,
            ..MountFlags::default()
        };
        self.in_use.store(true, Ordering::Relaxed);
        self.vfs()
            .mount(guest.as_bytes(), fs, flags, true)
            .await
            .map(drop)
            .map_err(errno_to_io)
    }

    /// Maps the host directory `host_dir` to `guest`. The guest cannot reach anything outside
    /// `host_dir`; files the host user owns appear as owned by the machine's user.
    pub async fn mount_host_dir(
        &self,
        guest: &str,
        host_dir: &Path,
        read_only: bool,
    ) -> io::Result<()> {
        let options = wasixv2_vfs_host::Options {
            read_only,
            owner: self.owner,
        };
        let fs = wasixv2_vfs_host::open(host_dir, self.host.clone(), options)?;
        self.mount(guest, fs, read_only).await
    }

    /// Mounts the volume `volume` of the webc v3 package `reader` at `guest`, sharing the
    /// reader's buffer. With `writable`, the volume is the lower layer of an overlay whose upper
    /// layer is in memory, so changes work and are lost with the machine; otherwise the mount is
    /// read-only. Objects in the volume appear owned by the machine's user.
    pub async fn mount_package_volume(
        &self,
        guest: &str,
        reader: &OwnedReader,
        volume: &str,
        writable: bool,
    ) -> io::Result<()> {
        let (uid, gid) = self.owner;
        let lower: Arc<dyn Filesystem> =
            Arc::new(PackageFs::with_owner(reader, volume, uid, gid).map_err(io::Error::other)?);
        let fs = if writable {
            let upper: Arc<dyn Filesystem> = Arc::new(MemFs::new(
                Arc::new(HostClock(self.host.clone())),
                0o755,
                uid,
                gid,
            ));
            OverlayFs::new(Some(upper), vec![lower])
        } else {
            lower
        };
        self.mount(guest, fs, !writable).await
    }

    /// Makes new processes start in `dir`, an absolute guest path.
    pub fn set_working_dir(&mut self, dir: impl Into<String>) {
        self.working_dir = Some(dir.into());
    }

    /// Starts `wasm` as a new process with no arguments or environment and waits for it to exit.
    /// Returns its exit status.
    pub async fn run(&self, wasm: &[u8], stdio: Stdio) -> Result<i32, RunError> {
        let config = ProcessConfig {
            args: Vec::new(),
            env: Vec::new(),
            stdio,
        };
        self.run_process(wasm, config).await
    }

    /// Sends the signal `sig` to every process started with [`Machine::run`],
    /// [`Machine::run_process`], [`Machine::run_module`] or [`Machine::run_file`] that is still
    /// running; see [`Signaller::signal`].
    pub fn signal(&self, sig: i32) -> io::Result<()> {
        self.signaller().signal(sig)
    }

    /// A handle that signals this machine's processes from any thread, for an embedder that
    /// forwards the signals of its host process.
    pub fn signaller(&self) -> Signaller {
        // A new root would boot a kernel the signaller does not reach.
        self.in_use.store(true, Ordering::Relaxed);
        Signaller {
            syscalls: self.syscalls.clone(),
            running: self.running.clone(),
        }
    }

    /// Starts `wasm` as a new process and waits for it to exit. Returns its exit status: that of
    /// `exit_group`, or 128 + the signal that killed it, a trap of the guest included.
    pub async fn run_process(&self, wasm: &[u8], config: ProcessConfig) -> Result<i32, RunError> {
        let engine = Engine::default();
        let module = load(&engine, wasm)?;
        self.run_module(&engine, &module, config).await
    }

    /// Starts `module`, compiled by `engine` with [`load`], as a new process and waits for it to
    /// exit. Returns its exit status. Threads are compiled by `engine` too, and the images the
    /// process `execve`s by the machine's [`Loader`], so an embedder picks the compiler and can
    /// cache compiled modules.
    pub async fn run_module(
        &self,
        engine: &Engine,
        module: &Module,
        config: ProcessConfig,
    ) -> Result<i32, RunError> {
        let (task, _running) = self.start(config).await?;
        self.run_image(Store::new(engine.clone()), module, task)
            .await
    }

    /// Starts the program at `path`, a guest path resolved from the working directory, as a
    /// new process and waits for it to exit, the way Linux starts init (`kernel_execve`): the
    /// process `execve`s `path` with `config`'s arguments and environment, so everything
    /// `execve` checks and does applies. A `#!` script runs its interpreter, the image runs
    /// with the ABI it targets, compiled by the machine's [`Loader`] (given `engine` as the
    /// engine the process ran on), and `/proc/self/exe` names the file. Past the `execve`, the
    /// process runs as any other's new image does: one that does not compile dies of SIGSEGV.
    /// Returns the exit status, or [`RunError::Execve`] with the errno `execve` failed with;
    /// the process is gone then.
    pub async fn run_file(
        &self,
        engine: &Engine,
        path: &str,
        config: ProcessConfig,
    ) -> Result<i32, RunError> {
        let bytes = |strings: &[String]| strings.iter().map(|s| s.as_bytes().to_vec()).collect();
        let (args, env) = (bytes(&config.args), bytes(&config.env));
        let (task, _running) = self.start(config).await?;
        let kernel = &self.syscalls.inner;
        let image = match kernel.exec(task, path.as_bytes(), args, env).await {
            Ok(image) => image,
            Err(errno) => {
                // Killed before its image started (`execve` of a dying group fails): it ends
                // as the kill says.
                if let Some(status) = killed(kernel, task) {
                    return Ok(status);
                }
                abandon(kernel, task);
                return Err(RunError::Execve {
                    path: path.to_string(),
                    err: errno_to_io(errno),
                });
            }
        };
        let (syscalls, abis) = (self.syscalls.clone(), self.abis.clone());
        let run = wasixv2_exec::run_exec(
            engine.clone(),
            image,
            syscalls,
            task,
            self.host.clone(),
            abis,
        );
        Ok(run.await?)
    }

    /// Spawns the process `config` describes, in the working directory, and keeps it on
    /// [`Machine::signal`]'s list while the returned guard lives.
    async fn start(&self, config: ProcessConfig) -> Result<(TaskId, Running<'_>), RunError> {
        self.in_use.store(true, Ordering::Relaxed);
        #[cfg(feature = "wasix1")]
        self.resolve_wasix1_commands().await;
        let task = self.syscalls.inner.spawn(config);
        self.running.lock().unwrap().push(task);
        let running = Running {
            machine: self,
            task,
        };
        if let Some(dir) = &self.working_dir {
            self.syscalls
                .inner
                .set_working_dir(task, dir.as_bytes())
                .await
                .map_err(|errno| RunError::WorkingDir(errno_to_io(errno)))?;
        }
        Ok((task, running))
    }

    async fn run_image(
        &self,
        store: Store,
        module: &Module,
        task: TaskId,
    ) -> Result<i32, RunError> {
        let host = self.host.clone();
        let (syscalls, abis) = (self.syscalls.clone(), self.abis.clone());
        Ok(wasixv2_exec::run_with_abis(store, module, syscalls, task, host, abis).await?)
    }
}

/// Sends signals to the processes a [`Machine`] runs, from any thread; see
/// [`Machine::signaller`].
#[derive(Clone)]
pub struct Signaller {
    syscalls: Arc<Layered<Trace, Kernel>>,
    running: Arc<Mutex<Vec<TaskId>>>,
}

impl Signaller {
    /// Sends the signal `sig` to every process started with [`Machine::run`],
    /// [`Machine::run_process`], [`Machine::run_module`] or [`Machine::run_file`] that is still
    /// running, as a signal from outside the machine
    /// (`SI_USER` with `si_pid` 0), as `kill` of the host process does. It goes through the
    /// kernel's normal signal generation: the guest's handler runs if it installed one, and
    /// otherwise the default action does, so e.g. an unhandled `SIGTERM` ends the process with
    /// status 128 + 15. A busy guest notices it at its next preemption point.
    ///
    /// Fails with `EINVAL` for a signal number outside 1..=64.
    pub fn signal(&self, sig: i32) -> io::Result<()> {
        self.each(sig, |kernel, task, sig| kernel.kill(task, sig))
    }

    /// Sends `sig` to the process groups of those processes, as a terminal sends the signals of
    /// `^C` and `^\` to its foreground process group, so the children of a guest shell get
    /// them too.
    pub fn signal_foreground(&self, sig: i32) -> io::Result<()> {
        self.each(sig, |kernel, task, sig| {
            kernel.kill_process_group(task, sig)
        })
    }

    /// Whether a `sig` sent with [`Signaller::signal_foreground`] is still pending, not yet
    /// taken by any process of those groups.
    pub fn foreground_pending(&self, sig: i32) -> bool {
        let running = self.running.lock().unwrap().clone();
        running.into_iter().any(|task| {
            self.syscalls
                .inner
                .process_group_pending(task, wasixv2_abi::Signal(sig))
        })
    }

    fn each(
        &self,
        sig: i32,
        send: impl Fn(&Kernel, TaskId, wasixv2_abi::Signal) -> Result<(), Errno>,
    ) -> io::Result<()> {
        if !(1..=64).contains(&sig) {
            return Err(errno_to_io(Errno::EINVAL));
        }
        let running = self.running.lock().unwrap().clone();
        for task in running {
            match send(&self.syscalls.inner, task, wasixv2_abi::Signal(sig)) {
                // It exited meanwhile.
                Ok(()) | Err(Errno::ESRCH) => {}
                Err(errno) => return Err(errno_to_io(errno)),
            }
        }
        Ok(())
    }
}

/// A kernel on `root` with the machine's network, user and names, whose `execve` starts the
/// images `abis` serve; its own filesystems are mounted next ([`mount_kernel_filesystems`]).
fn boot(
    host: &Arc<dyn Host>,
    root: Arc<dyn Filesystem>,
    network: &Arc<Net>,
    user: &Credentials,
    uname: &Uname,
    abis: Arc<Abis<Layered<Trace, Kernel>>>,
) -> Kernel {
    Kernel::new(host.clone(), root)
        .with_binfmt(abis)
        .with_network(network.clone())
        .with_shm(shm_filesystem(host))
        .with_credentials(user.clone())
        .with_uname(uname)
}

/// The package commands WASIX v1 programs start with their defaults: the names registered,
/// until the first process resolves them to the files in `files`, which `Wasix1` reads.
#[cfg(feature = "wasix1")]
#[derive(Default)]
struct Wasix1Commands {
    names: Mutex<Vec<(String, CommandDefaults)>>,
    files: Arc<std::sync::RwLock<wasixv2_wasix1::Commands>>,
}

/// The guest ABIs programs may target, `wasixv2` first, and how their images compile.
fn abis(
    loader: Arc<dyn Loader>,
    #[cfg(feature = "wasix1")] wasix1: &Wasix1Commands,
) -> Abis<Layered<Trace, Kernel>> {
    let mut abis = Abis::new();
    abis.set_loader(loader);
    abis.push(Arc::new(Wasixv2));
    #[cfg(feature = "wasix1")]
    abis.push(Arc::new(wasixv2_wasix1::Wasix1::with_commands(
        wasix1.files.clone(),
    )));
    abis
}

/// When an exit is queued for `task`, a process that never ran an image: its status, and the
/// task goes.
fn killed(kernel: &Kernel, task: TaskId) -> Option<i32> {
    use wasixv2_syscalls::{Control, GuestAction};

    let mut status = None;
    while let Some(action) = kernel.take_pending(task) {
        if let GuestAction::Exit(exit) = action {
            status.get_or_insert(exit);
        }
    }
    status.inspect(|_| kernel.release(task))
}

/// Ends `task`, a process that never ran an image, with SIGKILL, and lets it go.
fn abandon(kernel: &Kernel, task: TaskId) {
    use wasixv2_syscalls::Control;

    kernel.fault(task, wasixv2_abi::Signal::SIGKILL.0);
    kernel.release(task);
}

/// Takes a process off [`Machine::signal`]'s list however `run_process` ends.
struct Running<'a> {
    machine: &'a Machine,
    task: TaskId,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.machine
            .running
            .lock()
            .unwrap()
            .retain(|task| *task != self.task);
    }
}

#[cfg(all(feature = "host-network", unix))]
fn default_network() -> Arc<Net> {
    let backend = match wasixv2_vnet_host::HostBackend::new() {
        Ok(backend) => backend,
        Err(err) => {
            tracing::warn!(%err, "no host network backend; the guest network is loopback-only");
            return Net::new(NetConfig::default(), None);
        }
    };
    let mut config = NetConfig::host_default();
    let host_resolv = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    config.nameservers =
        wasixv2_vnet_host::host_nameservers(&host_resolv, config.host_loopback_alias);
    Net::new(config, Some(backend))
}

#[cfg(not(all(feature = "host-network", unix)))]
fn default_network() -> Arc<Net> {
    Net::new(NetConfig::default(), None)
}

/// The guest's `/etc/hosts`: `localhost` resolves without DNS, as on every Linux system.
const HOSTS: &str = "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\n";
/// The kernel's nodename.
const HOSTNAME: &str = "localhost\n";

/// The kernel's internal tmpfs for `memfd_create` (`shm_mnt`): its root is root's, mode 1777,
/// and like Linux's internal mount it has no size limit of its own.
fn shm_filesystem(host: &Arc<dyn Host>) -> Arc<dyn Filesystem> {
    Arc::new(MemFs::new(Arc::new(HostClock(host.clone())), 0o1777, 0, 0))
}

/// The default root filesystem: a memory filesystem whose top directory `user` owns, laid out
/// as a small Linux system. It has empty `/bin`, `/usr/bin` and `/usr/lib` for packages and
/// mounts to fill, `/root` and the user's home `/home/user`, and in `/etc` `passwd` and `group`
/// naming root and the user, `hostname`, `hosts` and, when there are name servers,
/// `resolv.conf`. `/proc`, `/dev` and `/tmp` are mount points.
pub fn default_root(
    host: &Arc<dyn Host>,
    resolv_conf: &str,
    user: &Credentials,
) -> Arc<dyn Filesystem> {
    let (uid, gid) = (user.uid(), user.gid());
    let root: Arc<dyn Filesystem> = Arc::new(MemFs::new(
        Arc::new(HostClock(host.clone())),
        0o755,
        uid,
        gid,
    ));
    let vfs = Vfs::new(root.clone());
    let cred = Cred::root();
    let top = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &top,
        umask: 0,
        task: 0,
    };
    let layout = async {
        let dirs = [
            "/bin",
            "/usr/bin",
            "/usr/lib",
            "/etc",
            "/home/user",
            "/proc",
            "/dev",
            "/tmp",
        ];
        for dir in dirs {
            vfs.mkdir_all(&cx, dir.as_bytes()).await?;
        }
        vfs.mkdir(&cx, &top, b"/root", 0o700).await?;
        let home = vfs
            .lookup(&cx, &top, b"/home/user", LookupFlags::follow())
            .await?;
        vfs.chown(&cx, &home, Some(uid), Some(gid)).await?;
        let (passwd, group) = (passwd(uid, gid), group(gid));
        let mut files = vec![
            ("/etc/passwd", passwd.as_str()),
            ("/etc/group", group.as_str()),
            ("/etc/hostname", HOSTNAME),
            ("/etc/hosts", HOSTS),
        ];
        if !resolv_conf.is_empty() {
            files.push(("/etc/resolv.conf", resolv_conf));
        }
        let how = wasixv2_vfs::OpenHow {
            flags: wasixv2_abi::OpenFlags::O_CREAT | wasixv2_abi::OpenFlags::O_WRONLY,
            mode: 0o644,
            lookup: LookupFlags::follow(),
        };
        for (path, contents) in files {
            let file = vfs.open(&cx, &top, path.as_bytes(), &how).await?;
            file.write_at(contents.as_bytes(), 0).await?;
        }
        Ok::<_, Errno>(())
    };
    now(layout).expect("a new memory filesystem takes the default layout");
    root
}

/// `/etc/passwd`: root, and the user processes run as unless that is root.
fn passwd(uid: u32, gid: u32) -> String {
    let mut passwd = String::from("root:x:0:0:root:/root:/bin/sh\n");
    if uid != 0 {
        passwd += &format!("user:x:{uid}:{gid}:user:/home/user:/bin/sh\n");
    }
    passwd
}

/// `/etc/group`: root's group, and the user's unless that is root's.
fn group(gid: u32) -> String {
    let mut group = String::from("root:x:0:\n");
    if gid != 0 {
        group += &format!("user:x:{gid}:\n");
    }
    group
}

/// Mounts the kernel's filesystems as a Linux system has them: `/proc`, `/dev` (a memory
/// filesystem with the device nodes, as devtmpfs is, and the conventional links into
/// `/proc/self/fd`), and memory filesystems at `/dev/shm` for POSIX shared memory and at
/// `/tmp`, world-writable and sticky.
async fn mount_kernel_filesystems(kernel: &Kernel, host: &Arc<dyn Host>) {
    let vfs = kernel.vfs();
    let tmpfs = |mode| -> Arc<dyn Filesystem> {
        Arc::new(MemFs::new(Arc::new(HostClock(host.clone())), mode, 0, 0))
    };
    let flags = |nodev, noexec| MountFlags {
        nosuid: true,
        nodev,
        noexec,
        ..MountFlags::default()
    };
    let cred = Cred::root();
    let top = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &top,
        umask: 0,
        task: 0,
    };
    let mount = |at: &'static str, fs, flags| async move {
        match vfs.mount(at.as_bytes(), fs, flags, true).await {
            Ok(_) => true,
            Err(errno) => {
                tracing::warn!(
                    at,
                    ?errno,
                    "cannot mount a kernel filesystem; it is left out"
                );
                false
            }
        }
    };
    let mounts = async {
        mount("/proc", kernel.procfs(), flags(true, true)).await;
        if mount("/dev", tmpfs(0o755), flags(false, false)).await {
            kernel.create_device_nodes(b"/dev").await?;
            for (link, target) in [
                ("/dev/fd", "/proc/self/fd"),
                ("/dev/stdin", "/proc/self/fd/0"),
                ("/dev/stdout", "/proc/self/fd/1"),
                ("/dev/stderr", "/proc/self/fd/2"),
            ] {
                vfs.symlink(&cx, target.as_bytes(), &top, link.as_bytes())
                    .await?;
            }
            mount("/dev/shm", tmpfs(0o1777), flags(true, false)).await;
        }
        mount("/tmp", tmpfs(0o1777), flags(true, false)).await;
        Ok::<_, Errno>(())
    };
    mounts
        .await
        .expect("the kernel's filesystems take their layout");
}

/// Runs `future`, which never waits: memory filesystems and the kernel's own complete every
/// operation at once.
fn now<T>(future: impl Future<Output = T>) -> T {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("memory filesystem operations complete at once"),
    }
}

fn errno_to_io(errno: Errno) -> io::Error {
    io::Error::from_raw_os_error(errno.0)
}

/// [`is_wasixv2_module`] for a compiled module, e.g. one deserialized from an artifact.
pub fn targets_wasixv2(module: &Module) -> bool {
    module
        .imports()
        .any(|import| import.module() == wasixv2_exec::IMPORT_MODULE)
}
