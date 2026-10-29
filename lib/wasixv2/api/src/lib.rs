//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

#![recursion_limit = "256"]

use std::{
    collections::HashSet,
    future::Future,
    io,
    path::Path,
    pin::pin,
    sync::{
        Arc, Mutex, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

pub use shared_buffer::OwnedBuffer;
use wasixv2_exec::{Abis, Wasixv2};
pub use wasixv2_exec::{
    Compile, ExecError, Image, ImageId, InstrumentError, LoadError, Loader, instrument,
    is_preemptible, is_wasixv2_module, load, warn_not_preemptible,
};
#[cfg(not(target_family = "wasm"))]
pub use wasixv2_host::BlockingSource;
#[cfg(feature = "tokio")]
pub use wasixv2_host::TokioHost;
#[cfg(feature = "web")]
pub use wasixv2_host::WebHost;
pub use wasixv2_host::{
    ByteSink, ByteSource, Console, EmptySource, Executor, FnSink, Host, LocalFuture, ThreadStart,
    WindowSize,
};
pub use wasixv2_kernel::{Credentials, PowerRequest, ProcessConfig, Stdio, Uname, UnameError};
use wasixv2_kernel::{FilesystemType, FsParam, HostClock, Kernel, SyscallEntry, Timekeeper};
use wasixv2_pages::{Provider, SharedPages};
use wasixv2_syscalls::{Layered, TaskId};
use wasixv2_trace::Trace;
pub use wasixv2_vfs::Filesystem;
use wasixv2_vfs::{Cred, Ctx, Errno, LookupFlags, MountFlags, NAME_MAX, Vfs};
use wasixv2_vfs_mem::{Limits, MemFs, QuotaLimits};
use wasixv2_vfs_overlay::{MAX_LOWERS, OverlayFs};
use wasixv2_vfs_package::PackageFs;
pub use wasixv2_vnet::{Net, NetConfig};
#[cfg(all(feature = "host-network", unix))]
pub use wasixv2_vnet_host::{VsockConfig, VsockRoute};
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
    #[error("cannot open the console: {0}")]
    Console(io::Error),
    #[error("cannot start the process: {0}")]
    Spawn(io::Error),
}

/// The directories [`Machine::install_commands`] puts commands in, relative to the root.
const COMMAND_DIRS: [&str; 2] = ["bin", "usr/bin"];

/// The syscalls programs call: through the kernel's syscall entry (seccomp), traced, into the
/// kernel.
type Stack = Layered<SyscallEntry<Trace>, Kernel>;

fn stack(kernel: Kernel) -> Stack {
    Layered::new(SyscallEntry(Trace), kernel)
}

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and laid out as a small Linux system (see
/// [`default_root`]); [`MachineBuilder::root`] takes another, and [`Machine::mount_root_layers`]
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
    syscalls: Arc<Stack>,
    /// The guest ABIs programs may target: `wasixv2`, then WASIX v1 with the `wasix1` feature.
    abis: Arc<Abis<Stack>>,
    /// The package commands WASIX v1 programs start with their defaults.
    #[cfg(feature = "wasix1")]
    wasix1: Wasix1Commands,
    network: Arc<Net>,
    /// The machine's clocks, whose wall clock its kernel owns: what every kernel and
    /// filesystem of the machine reads the time from, as [`Machine::host`] is.
    time: Arc<Timekeeper>,
    /// [`Machine::time`] as a host, over the embedder's.
    host: Arc<dyn Host>,
    /// The uid and gid processes start as and filesystems are owned by.
    owner: (u32, u32),
    /// Where new processes start; `/` if unset.
    working_dir: Option<String>,
    /// The processes [`Machine::run`], [`Machine::run_process`], [`Machine::run_module`] and
    /// [`Machine::run_file`] started that are still running.
    running: Arc<Mutex<Vec<TaskId>>>,
    /// Who the processes start as, what `uname` reports and whether the kernel has an init,
    /// for a kernel booted on a new root.
    user: Credentials,
    uname: Uname,
    init: bool,
    /// The root filesystem's writable top layer: the [`default_root`] or the embedder's.
    root_upper: Arc<dyn Filesystem>,
    /// Read-only layers under `root_upper`, topmost first; see [`Machine::mount_root_layers`].
    root_lowers: Vec<Arc<dyn Filesystem>>,
    /// Set by the first mount, process or [`Signaller`]: from then on the root filesystem is
    /// fixed.
    in_use: AtomicBool,
    /// The host's console, and which descriptors of a new process are on it.
    console: Option<(Console, [bool; 3])>,
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
    init: bool,
    /// The embedder's loader; the ABIs' own default otherwise.
    loader: Option<Arc<dyn Loader>>,
    console: Option<(Console, [bool; 3])>,
    dns: Dns,
    #[cfg(all(feature = "host-network", unix))]
    vsock: Option<VsockConfig>,
}

/// Where the machine's programs resolve names: what the guest's `/etc/resolv.conf` names, on
/// a machine that reaches the host's network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dns {
    /// The machine's own DNS server, at 10.0.2.3 on the uplink as slirp's, which answers from
    /// the host's resolver: the host's hosts file, `getaddrinfo` and name servers, whatever
    /// the host's resolver reaches (a local stub, a VPN's servers).
    #[default]
    Builtin,
    /// The host's name servers, asked directly: those of the host's `/etc/resolv.conf` the
    /// guest reaches (a server on the host's `127.0.0.1` through the gateway alias), else the
    /// upstream servers of systemd-resolved.
    Host,
    /// None: names resolve from `/etc/hosts` only.
    None,
}

impl MachineBuilder {
    /// A machine whose clocks, timers, entropy and executor come from `host`.
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            user: Credentials::default(),
            uname: Uname::default(),
            root: None,
            init: true,
            loader: None,
            console: None,
            dns: Dns::default(),
            #[cfg(all(feature = "host-network", unix))]
            vsock: None,
        }
    }

    /// Makes the host network also the machine's vsock host side: the machine is a vsock
    /// guest with `vsock.cid`, and its connections to the host's CIDs go where `vsock` routes
    /// them. Without it the machine has only vsock's loopback, as a Linux host does.
    #[cfg(all(feature = "host-network", unix))]
    pub fn vsock(mut self, vsock: VsockConfig) -> Self {
        self.vsock = Some(vsock);
        self
    }

    /// Where the machine's programs resolve names ([`Dns::Builtin`] by default).
    pub fn dns(mut self, dns: Dns) -> Self {
        self.dns = dns;
        self
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

    /// Whether the machine boots with an init, pid 1, as Linux does; on by default. Init is the
    /// kernel's own: it runs no guest code, every program the machine starts is its child, it
    /// reaps the orphans handed to it, and no signal kills it. Without one, programs start as
    /// pid 1 with no parent (`getppid` is 0), and orphans have no reaper and are reaped as they
    /// exit.
    pub fn init(mut self, init: bool) -> Self {
        self.init = init;
        self
    }

    /// What compiles the images processes `execve`, and the program [`Machine::run_file`]
    /// starts: by default [`Compile`], with the engine the process ran on and no cache. An
    /// embedder's loader picks the engine per image and caches compiled modules, telling images
    /// apart by [`Image::identity`] or their bytes. With the machine's ABIs it is the machine's
    /// set of binary formats: what its [`Loader::probe`] refuses, or no ABI serves, fails
    /// `execve` with `ENOEXEC`.
    pub fn loader(mut self, loader: Arc<dyn Loader>) -> Self {
        self.loader = Some(loader);
        self
    }

    /// Gives the machine the host's terminal as its console, a terminal device (`/dev/hvc0`)
    /// whose line discipline is the kernel's: `console` is the host's side of it, in raw mode.
    /// Each program the machine starts has the descriptors `fds` names of 0, 1 and 2 open on
    /// it instead of its [`Stdio`] streams, as getty opens a terminal for a login, and starts
    /// a session that has it as its controlling terminal unless another session has it, so a
    /// shell gets job control. [`Signaller::console_resize`] and
    /// [`Signaller::console_hangup`] pass on what happens to the host's terminal.
    pub fn console(mut self, console: Console, fds: [bool; 3]) -> Self {
        self.console = Some((console, fds));
        self
    }

    pub fn build(self) -> Machine {
        let Self {
            host,
            user,
            uname,
            root,
            init,
            loader,
            console,
            dns,
            #[cfg(all(feature = "host-network", unix))]
            vsock,
        } = self;
        let owner = (user.uid(), user.gid());
        // The machine's wall clock is its own: setting it leaves the embedder's alone.
        let time = Timekeeper::new(host);
        let host: Arc<dyn Host> = time.clone();
        #[cfg(all(feature = "host-network", unix))]
        let network = default_network(dns, vsock);
        #[cfg(not(all(feature = "host-network", unix)))]
        let network = default_network(dns);
        let root = root
            .unwrap_or_else(|| default_root(&host, &network.resolv_conf(), &user, uname.release()));
        #[cfg(feature = "wasix1")]
        let wasix1 = Wasix1Commands::default();
        let abis = Arc::new(abis(
            loader,
            #[cfg(feature = "wasix1")]
            &wasix1,
        ));
        let kernel = boot(
            &time,
            root.clone(),
            &network,
            &user,
            &uname,
            init,
            console.as_ref().map(|(console, _)| console),
            abis.clone(),
        );
        now(mount_kernel_filesystems(&kernel, &host));
        let syscalls = Arc::new(stack(kernel));
        Helpers::install(&syscalls, &abis, &host);
        Machine {
            syscalls,
            abis,
            #[cfg(feature = "wasix1")]
            wasix1,
            network,
            time,
            host,
            owner,
            working_dir: None,
            running: Arc::default(),
            user,
            uname,
            init,
            root_upper: root,
            root_lowers: Vec::new(),
            in_use: AtomicBool::new(false),
            console,
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

    /// The volume `volume` of the webc v3 package `reader` as a [`Layer`], sharing the reader's
    /// buffer. Objects in the volume appear owned by the machine's user. Making it checks every
    /// directory of the volume, so mount clones of one layer rather than making it again.
    pub fn package_layer(&self, reader: &OwnedReader, volume: &str) -> io::Result<Layer> {
        let (uid, gid) = self.owner;
        let fs = PackageFs::with_owner(reader, volume, uid, gid).map_err(io::Error::other)?;
        Ok(Layer(Arc::new(fs)))
    }

    /// The host directory `host_dir` as a [`Layer`], which the guest sees but never changes;
    /// files the host user owns appear as owned by the machine's user.
    pub fn host_dir_layer(&self, host_dir: &Path) -> io::Result<Layer> {
        let options = wasixv2_vfs_host::Options {
            read_only: true,
            owner: self.owner,
        };
        Ok(Layer(wasixv2_vfs_host::open(
            host_dir,
            self.host.clone(),
            options,
        )?))
    }

    /// `commands`, each a name and the program's bytes, as a [`Layer`] that holds the
    /// executable files `bin/<name>` and `usr/bin/<name>` of each, as a package tree's commands
    /// are, or `None` when there are no commands. Every file is a regular `0755` file owned by
    /// the machine's user, whose contents are the bytes, shared with the buffer; `execve` loads
    /// it like any other file. Of several commands with one name, the first wins.
    ///
    /// Fails with `EINVAL` for a name that cannot be a file name (empty, `.`, `..`, or with
    /// `/` or NUL), and `ENAMETOOLONG` for one longer than `NAME_MAX`.
    pub fn commands_layer(
        &self,
        commands: impl IntoIterator<Item = (String, OwnedBuffer)>,
    ) -> io::Result<Option<Layer>> {
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
        if files.is_empty() {
            return Ok(None);
        }
        let (uid, gid) = self.owner;
        let layer = PackageFs::from_files(files, uid, gid).map_err(io::Error::other)?;
        Ok(Some(Layer(Arc::new(layer))))
    }

    /// Stacks `layers` under the root filesystem's top layer (the [`default_root`] or the
    /// embedder's root), which holds `/etc` and every change: each layer over the ones before
    /// it, and all of them over the layers earlier calls added. The kernel's filesystems are
    /// mounted on the new root as on the old one, once per call, so a root of several layers
    /// is best stacked in one.
    ///
    /// The root can only change on a fresh machine: this fails with `EBUSY` after anything was
    /// mounted or run, or a [`Signaller`] was handed out.
    pub async fn mount_root_layers(
        &mut self,
        layers: impl IntoIterator<Item = Layer>,
    ) -> io::Result<()> {
        if *self.in_use.get_mut() {
            return Err(errno_to_io(Errno::EBUSY));
        }
        let layers: Vec<Layer> = layers.into_iter().collect();
        if layers.is_empty() {
            return Ok(());
        }
        if self.root_lowers.len() + layers.len() > MAX_LOWERS {
            return Err(io::Error::other("too many layers stacked under the root"));
        }
        for Layer(lower) in layers {
            self.root_lowers.insert(0, lower);
        }
        let root = OverlayFs::new(Some(self.root_upper.clone()), self.root_lowers.clone());
        // Nothing lives in the kernel yet, so a new one on the new root replaces it, with the
        // kernel's filesystems mounted afresh; a host directory's layer answers asynchronously.
        let kernel = boot(
            &self.time,
            root,
            &self.network,
            &self.user,
            &self.uname,
            self.init,
            self.console.as_ref().map(|(console, _)| console),
            self.abis.clone(),
        );
        mount_kernel_filesystems(&kernel, &self.host).await;
        self.syscalls = Arc::new(stack(kernel));
        Helpers::install(&self.syscalls, &self.abis, &self.host);
        Ok(())
    }

    /// Makes the volume `volume` of the webc v3 package `reader` a read-only layer of the root
    /// filesystem ([`Machine::package_layer`], stacked with [`Machine::mount_root_layers`]).
    /// This is how a package's volume at `/` is mounted.
    pub async fn mount_root_lower(&mut self, reader: &OwnedReader, volume: &str) -> io::Result<()> {
        let layer = self.package_layer(reader, volume)?;
        self.mount_root_layers([layer]).await
    }

    /// Makes the host directory `host_dir` a read-only layer of the root filesystem
    /// ([`Machine::host_dir_layer`], stacked with [`Machine::mount_root_layers`]): the guest
    /// sees it through the root but never changes it, and changes stay in the top layer. This
    /// is how a host directory mapped to `/` is mounted, since nothing can be mounted over the
    /// root.
    pub async fn mount_root_lower_host_dir(&mut self, host_dir: &Path) -> io::Result<()> {
        let layer = self.host_dir_layer(host_dir)?;
        self.mount_root_layers([layer]).await
    }

    /// Makes each of `commands` an executable file at `/bin/<name>` and `/usr/bin/<name>`
    /// ([`Machine::commands_layer`], stacked with [`Machine::mount_root_layers`]), so `/bin`
    /// and `/usr/bin` keep what the layers under it (package volumes) have there and a command
    /// hides a file of its name; the guest can still add, replace and remove entries, and the
    /// changes stay in memory. No commands add no layer.
    pub async fn install_commands(
        &mut self,
        commands: impl IntoIterator<Item = (String, OwnedBuffer)>,
    ) -> io::Result<()> {
        let layer = self.commands_layer(commands)?;
        self.mount_root_layers(layer).await
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
            leases: None,
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

    /// Mounts `layer` at `guest` (an absolute guest path), creating missing directories on the
    /// way, over whatever is there. With `writable`, the layer is the lower layer of an overlay
    /// whose upper layer is in memory, so changes work and are lost with the machine; otherwise
    /// the mount is read-only. A layer mounted at several paths shares its files, which were
    /// made and checked once; each writable mount has changes of its own.
    pub async fn mount_layer(&self, guest: &str, layer: &Layer, writable: bool) -> io::Result<()> {
        let lower = layer.0.clone();
        let fs = if writable {
            let (uid, gid) = self.owner;
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

    /// Mounts the volume `volume` of the webc v3 package `reader` at `guest`
    /// ([`Machine::package_layer`], mounted with [`Machine::mount_layer`]).
    pub async fn mount_package_volume(
        &self,
        guest: &str,
        reader: &OwnedReader,
        volume: &str,
        writable: bool,
    ) -> io::Result<()> {
        let layer = self.package_layer(reader, volume)?;
        self.mount_layer(guest, &layer, writable).await
    }

    /// The environment of a login as the machine's user on the [`default_root`], for an
    /// embedder to start programs with: `HOME`, the user's home directory as its
    /// `/etc/passwd` names it, and `PATH`, musl's default search path, which takes in the
    /// directories commands are installed in. Processes start with only the environment
    /// they are given, as on Linux, so this is the embedder's to pass.
    pub fn default_env(&self) -> Vec<String> {
        vec![
            format!("HOME={}", home_dir(self.user.uid())),
            format!("PATH={DEFAULT_PATH}"),
        ]
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
    /// `exit_group`, or 128 + the signal that killed it, a trap of the guest included. Every run
    /// returns once the console's output, when the machine has one, reached the host.
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
        let (task, running) = self.start(config).await?;
        let status = self
            .run_image(Store::new(engine.clone()), module, task)
            .await;
        running.released();
        self.console_sent().await;
        status
    }

    /// Starts the program at `path`, a guest path resolved from the working directory, as a
    /// new process and waits for it to exit, the way Linux starts init (`kernel_execve`): the
    /// process `execve`s `path` with `config`'s arguments and environment, so everything
    /// `execve` checks and does applies. A `#!` script runs its interpreter, the image runs
    /// with the ABI it targets, compiled by the machine's [`Loader`] (given `engine` as the
    /// engine the process ran on), and `/proc/self/exe` names the file. Past the `execve`, the
    /// process runs as any other's new image does, and the images it `execve`s that do not
    /// start kill it with SIGSEGV. Returns the exit status; [`RunError::Execve`] with the errno
    /// `execve` failed with; or, when the program itself does not compile or instantiate,
    /// [`RunError::Exec`], as [`Machine::run_module`] fails. The process is gone then.
    pub async fn run_file(
        &self,
        engine: &Engine,
        path: &str,
        config: ProcessConfig,
    ) -> Result<i32, RunError> {
        let bytes = |strings: &[String]| strings.iter().map(|s| s.as_bytes().to_vec()).collect();
        let (args, env) = (bytes(&config.args), bytes(&config.env));
        let (task, running) = self.start(config).await?;
        let status = self.exec_file(engine, task, path, args, env).await;
        running.released();
        self.console_sent().await;
        status
    }

    /// A run's end: what the console holds for the host reaches it before the run returns, so
    /// an embedder that exits then loses none of it, such as the echo of the `^C` that ended
    /// the program ([`Kernel::console_wait_until_sent`], bounded as Linux's close of a console
    /// is).
    async fn console_sent(&self) {
        self.syscalls.inner.console_wait_until_sent().await;
    }

    /// The rest of [`Machine::run_file`] for `task`, which it releases however it ends.
    async fn exec_file(
        &self,
        engine: &Engine,
        task: TaskId,
        path: &str,
        args: Vec<Vec<u8>>,
        env: Vec<Vec<u8>>,
    ) -> Result<i32, RunError> {
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

    /// Ends every process of the machine, as powering it off does: each is killed with
    /// SIGKILL, and this returns once nothing of them runs any more, their threads and the
    /// processes they started included. The runs waiting for them end with 128 + 9. A busy
    /// guest dies at its next preemption point, so a precompiled module without them that
    /// never makes a syscall keeps this waiting. The machine stays usable: it runs what is
    /// started afterwards. Dropping the machine kills its processes too, without waiting.
    pub async fn shutdown(&self) {
        self.syscalls.inner.terminate().await;
    }

    /// The restart, halt or power-off a process asked for with `reboot(2)` (with
    /// `CAP_SYS_BOOT`, in the initial pid namespace), which ended every process as
    /// [`Machine::shutdown`] does; `None` if none did. The runs end with 128 + 9; an embedder
    /// reads this to tell a machine that was powered off from one whose processes were killed.
    pub fn power_request(&self) -> Option<PowerRequest> {
        self.syscalls.inner.power_request()
    }

    /// Spawns the process `config` describes, in the working directory, and keeps it on
    /// [`Machine::signal`]'s list while the returned guard lives; the guard kills it unless
    /// told that the run released it.
    async fn start(&self, config: ProcessConfig) -> Result<(TaskId, Running<'_>), RunError> {
        self.in_use.store(true, Ordering::Relaxed);
        #[cfg(feature = "wasix1")]
        self.resolve_wasix1_commands().await;
        let task = (self.syscalls.inner.spawn(config))
            .map_err(|errno| RunError::Spawn(errno_to_io(errno)))?;
        self.running.lock().unwrap().push(task);
        let running = Running {
            machine: self,
            task,
            released: false,
        };
        if let Some(dir) = &self.working_dir {
            self.syscalls
                .inner
                .set_working_dir(task, dir.as_bytes())
                .await
                .map_err(|errno| RunError::WorkingDir(errno_to_io(errno)))?;
        }
        if let Some((_, fds)) = &self.console {
            self.syscalls
                .inner
                .attach_console(task, *fds)
                .await
                .map_err(|errno| RunError::Console(errno_to_io(errno)))?;
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

/// A read-only filesystem for a [`Machine`]'s root or a mount, made once by
/// [`Machine::package_layer`], [`Machine::host_dir_layer`] or [`Machine::commands_layer`]:
/// stack it under the root with [`Machine::mount_root_layers`], or mount it at a path with
/// [`Machine::mount_layer`]. A clone shares the files, so a layer used in several places is
/// made and checked once.
#[derive(Clone)]
pub struct Layer(Arc<dyn Filesystem>);

/// Sends signals to the processes a [`Machine`] runs, from any thread; see
/// [`Machine::signaller`].
#[derive(Clone)]
pub struct Signaller {
    syscalls: Arc<Stack>,
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

    /// Sends `sig` to the foreground process group, as a terminal sends the signals of `^C`
    /// and `^\`: the console's, when the machine has one ([`MachineBuilder::console`]),
    /// otherwise the process groups of those processes, so the children of a guest shell get
    /// them too. Returns whether some process has it pending now: `false` when every process
    /// of the groups ignores it, so the kernel discarded it, as when no process runs. A
    /// pending signal is taken once [`Signaller::foreground_pending`] says so.
    pub fn signal_foreground(&self, sig: i32) -> io::Result<bool> {
        let kernel = &self.syscalls.inner;
        if kernel.has_console() {
            return match kernel.console_signal(wasixv2_abi::Signal(sig)) {
                Ok(pending) => Ok(pending),
                // Nothing is in the foreground.
                Err(Errno::ESRCH) => Ok(false),
                Err(errno) => Err(errno_to_io(errno)),
            };
        }
        let mut pending = false;
        self.each(sig, |kernel, task, sig| {
            pending |= kernel.kill_process_group(task, sig)?;
            Ok(())
        })?;
        Ok(pending)
    }

    /// Whether a `sig` sent with [`Signaller::signal_foreground`] is still pending, not yet
    /// taken by any process of those groups.
    pub fn foreground_pending(&self, sig: i32) -> bool {
        let kernel = &self.syscalls.inner;
        if kernel.has_console() {
            return kernel.console_signal_pending(wasixv2_abi::Signal(sig));
        }
        let running = self.running.lock().unwrap().clone();
        running.into_iter().any(|task| {
            self.syscalls
                .inner
                .process_group_pending(task, wasixv2_abi::Signal(sig))
        })
    }

    /// The host's terminal changed its size to `size`: the console's foreground process
    /// group gets SIGWINCH. Nothing without a console.
    pub fn console_resize(&self, size: WindowSize) {
        self.syscalls.inner.console_resize(size);
    }

    /// The host's terminal went away: the console hangs up, as a terminal whose line drops.
    /// The leader of the session it controls gets SIGHUP and SIGCONT, and the files open on
    /// it end. Nothing without a console.
    pub fn console_hangup(&self) {
        self.syscalls.inner.console_hangup();
    }

    fn each(
        &self,
        sig: i32,
        mut send: impl FnMut(&Kernel, TaskId, wasixv2_abi::Signal) -> Result<(), Errno>,
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

/// A kernel on `root` with the machine's network, user, names, init and console, whose
/// `execve` starts the images `abis` serve; its own filesystems are mounted next
/// ([`mount_kernel_filesystems`]). A network namespace a guest makes gets a fresh loopback-only
/// network, with no backend: nothing in it reaches the host. Its `lo` starts down, as
/// `copy_net_ns` leaves it on Linux, until the guest brings it up (`SIOCSIFFLAGS`,
/// `RTM_NEWLINK`).
#[allow(clippy::too_many_arguments)]
fn boot(
    time: &Arc<Timekeeper>,
    root: Arc<dyn Filesystem>,
    network: &Arc<Net>,
    user: &Credentials,
    uname: &Uname,
    init: bool,
    console: Option<&Console>,
    abis: Arc<Abis<Stack>>,
) -> Kernel {
    let host: Arc<dyn Host> = time.clone();
    let host = &host;
    let mut kernel = Kernel::with_timekeeper(time.clone(), root)
        .with_binfmt(abis)
        .with_network(network.clone())
        .with_network_factory(|| {
            let config = NetConfig {
                loopback_up: false,
                ..NetConfig::default()
            };
            Net::new(config, None)
        })
        .with_shm(shm_filesystem(host))
        .with_credentials(user.clone())
        .with_uname(uname);
    if init {
        kernel = kernel.with_init();
    }
    match console {
        Some(console) => kernel.with_console(console.clone()),
        None => kernel,
    }
}

/// The package commands WASIX v1 programs start with their defaults: the names registered,
/// until the first process resolves them to the files in `files`, which `Wasix1` reads.
#[cfg(feature = "wasix1")]
#[derive(Default)]
struct Wasix1Commands {
    names: Mutex<Vec<(String, CommandDefaults)>>,
    files: Arc<std::sync::RwLock<wasixv2_wasix1::Commands>>,
}

/// The guest ABIs programs may target, `wasixv2` first, and how their images compile: by
/// `loader`, or [`Compile`].
fn abis(
    loader: Option<Arc<dyn Loader>>,
    #[cfg(feature = "wasix1")] wasix1: &Wasix1Commands,
) -> Abis<Stack> {
    let mut abis = Abis::new();
    if let Some(loader) = loader {
        abis.set_loader(loader);
    }
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

/// Starts the programs the kernel runs itself (`call_usermodehelper`), as a core dump's pipe
/// helper: each runs as a program's image does, compiled on a default engine, and nothing
/// waits for it.
struct Helpers {
    syscalls: std::sync::Weak<Stack>,
    abis: Arc<Abis<Stack>>,
    host: Arc<dyn Host>,
}

impl Helpers {
    fn install(syscalls: &Arc<Stack>, abis: &Arc<Abis<Stack>>, host: &Arc<dyn Host>) {
        let helpers = Helpers {
            syscalls: Arc::downgrade(syscalls),
            abis: abis.clone(),
            host: host.clone(),
        };
        syscalls.inner.set_usermode_helper(Arc::new(helpers));
    }
}

impl wasixv2_kernel::UsermodeHelper for Helpers {
    fn start(&self, task: TaskId, image: Image) {
        let Some(syscalls) = self.syscalls.upgrade() else {
            return;
        };
        let (abis, host) = (self.abis.clone(), self.host.clone());
        let run = async move {
            let engine = Engine::default();
            // The helper's end is its own; a failure to start kills it as any image's does.
            let _ = wasixv2_exec::run_exec(engine, image, syscalls, task, host, abis).await;
        };
        self.host.executor().spawn_local(Box::pin(run));
    }
}

/// Ends `task`, a process whose image no longer runs, with SIGKILL, and lets it go.
fn abandon(kernel: &Kernel, task: TaskId) {
    use wasixv2_syscalls::Control;

    kernel.fault(task, wasixv2_abi::Signal::SIGKILL.0);
    kernel.release(task);
}

/// Takes a process off [`Machine::signal`]'s list however its run ends. A run whose future is
/// dropped before it released the process kills it, so nothing of it is left running.
struct Running<'a> {
    machine: &'a Machine,
    task: TaskId,
    released: bool,
}

impl Running<'_> {
    /// The run ended and released the process.
    fn released(mut self) {
        self.released = true;
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.machine
            .running
            .lock()
            .unwrap()
            .retain(|task| *task != self.task);
        if !self.released {
            abandon(&self.machine.syscalls.inner, self.task);
        }
    }
}

impl Drop for Machine {
    /// Kills the machine's processes; see [`Machine::shutdown`].
    fn drop(&mut self) {
        self.syscalls.inner.kill_all();
    }
}

#[cfg(all(feature = "host-network", unix))]
fn default_network(dns: Dns, vsock: Option<VsockConfig>) -> Arc<Net> {
    let resolver = wasixv2_vnet_host::ResolverConfig::default();
    let backend = match vsock {
        Some(vsock) => wasixv2_vnet_host::HostBackend::with_vsock(resolver, vsock),
        None => wasixv2_vnet_host::HostBackend::with_resolver(resolver),
    };
    let backend = match backend {
        Ok(backend) => backend,
        Err(err) => {
            tracing::warn!(%err, "no host network backend; the guest network is loopback-only");
            return Net::new(NetConfig::default(), None);
        }
    };
    let mut config = NetConfig::host_default();
    match dns {
        Dns::Builtin => {}
        Dns::Host => {
            let host_resolv = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
            let upstream =
                std::fs::read_to_string(wasixv2_vnet_host::SYSTEMD_RESOLVED_UPSTREAM).ok();
            config.dns_server.clear();
            config.nameservers = wasixv2_vnet_host::host_nameservers(
                &host_resolv,
                upstream.as_deref(),
                config.host_loopback_alias,
            );
        }
        Dns::None => {
            config.dns_server.clear();
            config.nameservers.clear();
        }
    }
    Net::new(config, Some(backend))
}

#[cfg(not(all(feature = "host-network", unix)))]
fn default_network(_: Dns) -> Arc<Net> {
    Net::new(NetConfig::default(), None)
}

/// The guest's `/etc/hosts`: `localhost` resolves without DNS, as on every Linux system.
const HOSTS: &str = "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\n";
/// The guest's `/etc/protocols`: the IP protocol numbers and names of IANA's registry, as a
/// Linux system's (Debian's `netbase`) lists them, for `getprotobyname` of a libc that reads
/// the file and for programs that parse it.
const PROTOCOLS: &str = "\
# Internet (IP) protocols, from IANA's protocol-numbers registry.
ip\t0\tIP\t\t# internet protocol, pseudo protocol number
hopopt\t0\tHOPOPT\t\t# IPv6 Hop-by-Hop Option [RFC1883]
icmp\t1\tICMP\t\t# internet control message protocol
igmp\t2\tIGMP\t\t# Internet Group Management
ggp\t3\tGGP\t\t# gateway-gateway protocol
ipencap\t4\tIP-ENCAP\t# IP encapsulated in IP (officially ``IP'')
st\t5\tST\t\t# ST datagram mode
tcp\t6\tTCP\t\t# transmission control protocol
egp\t8\tEGP\t\t# exterior gateway protocol
igp\t9\tIGP\t\t# any private interior gateway (Cisco)
pup\t12\tPUP\t\t# PARC universal packet protocol
udp\t17\tUDP\t\t# user datagram protocol
hmp\t20\tHMP\t\t# host monitoring protocol
xns-idp\t22\tXNS-IDP\t\t# Xerox NS IDP
rdp\t27\tRDP\t\t# \"reliable datagram\" protocol
iso-tp4\t29\tISO-TP4\t\t# ISO Transport Protocol class 4 [RFC905]
dccp\t33\tDCCP\t\t# Datagram Congestion Control Prot. [RFC4340]
xtp\t36\tXTP\t\t# Xpress Transfer Protocol
ddp\t37\tDDP\t\t# Datagram Delivery Protocol
idpr-cmtp 38\tIDPR-CMTP\t# IDPR Control Message Transport
ipv6\t41\tIPv6\t\t# Internet Protocol, version 6
ipv6-route 43\tIPv6-Route\t# Routing Header for IPv6
ipv6-frag 44\tIPv6-Frag\t# Fragment Header for IPv6
idrp\t45\tIDRP\t\t# Inter-Domain Routing Protocol
rsvp\t46\tRSVP\t\t# Reservation Protocol
gre\t47\tGRE\t\t# General Routing Encapsulation
esp\t50\tIPSEC-ESP\t# Encap Security Payload [RFC2406]
ah\t51\tIPSEC-AH\t# Authentication Header [RFC2402]
skip\t57\tSKIP\t\t# SKIP
ipv6-icmp 58\tIPv6-ICMP\t# ICMP for IPv6
ipv6-nonxt 59\tIPv6-NoNxt\t# No Next Header for IPv6
ipv6-opts 60\tIPv6-Opts\t# Destination Options for IPv6
rspf\t73\tRSPF CPHB\t# Radio Shortest Path First (officially CPHB)
vmtp\t81\tVMTP\t\t# Versatile Message Transport
eigrp\t88\tEIGRP\t\t# Enhanced Interior Routing Protocol (Cisco)
ospf\t89\tOSPFIGP\t\t# Open Shortest Path First IGP
ax.25\t93\tAX.25\t\t# AX.25 frames
ipip\t94\tIPIP\t\t# IP-within-IP Encapsulation Protocol
etherip\t97\tETHERIP\t\t# Ethernet-within-IP Encapsulation [RFC3378]
encap\t98\tENCAP\t\t# Yet Another IP encapsulation [RFC1241]
pim\t103\tPIM\t\t# Protocol Independent Multicast
ipcomp\t108\tIPCOMP\t\t# IP Payload Compression Protocol
vrrp\t112\tVRRP\t\t# Virtual Router Redundancy Protocol [RFC5798]
l2tp\t115\tL2TP\t\t# Layer Two Tunneling Protocol [RFC2661]
isis\t124\tISIS\t\t# IS-IS over IPv4
sctp\t132\tSCTP\t\t# Stream Control Transmission Protocol
fc\t133\tFC\t\t# Fibre Channel
mobility-header 135 Mobility-Header # Mobility Support for IPv6 [RFC3775]
udplite\t136\tUDPLite\t\t# UDP-Lite [RFC3828]
mpls-in-ip 137\tMPLS-in-IP\t# MPLS-in-IP [RFC4023]
manet\t138\t\t\t# MANET Protocols [RFC5498]
hip\t139\tHIP\t\t# Host Identity Protocol
shim6\t140\tShim6\t\t# Shim6 Protocol [RFC5533]
wesp\t141\tWESP\t\t# Wrapped Encapsulating Security Payload
rohc\t142\tROHC\t\t# Robust Header Compression
ethernet\t143\tEthernet\t# Ethernet encapsulation for SRv6 [RFC8986]
mptcp\t262\tMPTCP\t\t# Multipath TCP connection
";
/// The kernel's nodename.
const HOSTNAME: &str = "localhost\n";

/// The kernel's internal tmpfs for `memfd_create` (`shm_mnt`): its root is root's, mode 1777,
/// and like Linux's internal mount it has no size limit of its own. Its files live in host
/// shared memory, as `/dev/shm`'s do.
fn shm_filesystem(host: &Arc<dyn Host>) -> Arc<dyn Filesystem> {
    Arc::new(shared_tmpfs(host, 0o1777, 0, 0))
}

/// A tmpfs whose files processes share by mapping them: its regular files live in the host's
/// shared memory objects
/// ([`Host::shared_pages`]), so every `MAP_SHARED` mapping of one shows the same pages, as
/// tmpfs keeps its files in shmem. Only on Linux, where an object can change its size: a macOS
/// object's size is fixed once set, so there the files stay in the filesystem's own memory and
/// their mappings are copies kept in sync at syscalls.
fn shared_tmpfs(host: &Arc<dyn Host>, mode: u32, uid: u32, gid: u32) -> MemFs {
    let fs = MemFs::new(Arc::new(HostClock(host.clone())), mode, uid, gid);
    if cfg!(any(target_os = "linux", target_os = "android")) {
        return fs.with_shared_pages(Arc::new(HostPages(host.clone())));
    }
    fs
}

/// `tmpfs`, the filesystem type `mount -t tmpfs` makes: a memory filesystem whose files live in
/// host shared memory as [`shared_tmpfs`]'s, with `mm/shmem.c`'s options. It keeps track of
/// its instances to change their limits on remount.
struct Tmpfs {
    host: Arc<dyn Host>,
    instances: Mutex<Vec<Weak<MemFs>>>,
}

/// The options a tmpfs mount or remount names (`struct shmem_options`); `None` for one it
/// leaves out.
#[derive(Default)]
struct TmpfsOptions {
    quotas: QuotaLimits,
    max_bytes: Option<u64>,
    max_inodes: Option<u64>,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
}

impl Tmpfs {
    fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            instances: Mutex::default(),
        }
    }

    /// `shmem_parse_one` over `params`.
    fn options(params: &[FsParam<'_>]) -> Result<TmpfsOptions, Errno> {
        let memory = Limits::DEFAULT_MEMORY;
        let mut options = TmpfsOptions::default();
        for param in params {
            let value = || param.value.ok_or(Errno::EINVAL);
            match param.key {
                b"size" => {
                    let (size, rest) = memparse(value()?)?;
                    let size = match rest {
                        b"" => size,
                        b"%" => size.saturating_mul(memory) / 100,
                        _ => return Err(Errno::EINVAL),
                    };
                    options.max_bytes = Some(size.div_ceil(PAGE) * PAGE);
                }
                b"nr_blocks" => {
                    let blocks = memparse_all(value()?)?;
                    options.max_bytes = Some(blocks.checked_mul(PAGE).ok_or(Errno::EINVAL)?);
                }
                b"nr_inodes" => options.max_inodes = Some(memparse_all(value()?)?),
                b"quota" | b"usrquota" | b"grpquota" if param.value.is_none() => {
                    options.quotas.types |= match param.key {
                        b"usrquota" => 1,
                        b"grpquota" => 2,
                        _ => 3,
                    };
                }
                b"usrquota_block_hardlimit"
                | b"grpquota_block_hardlimit"
                | b"usrquota_inode_hardlimit"
                | b"grpquota_inode_hardlimit" => {
                    let limit = memparse_all(value()?)?;
                    if limit == 0 || limit > i64::MAX as u64 {
                        return Err(Errno::EINVAL);
                    }
                    let kind = usize::from(param.key.starts_with(b"grp"));
                    if param.key.ends_with(b"block_hardlimit") {
                        options.quotas.block_hard[kind] = limit;
                    } else {
                        options.quotas.inode_hard[kind] = limit;
                    }
                }
                b"mode" => {
                    let mode = std::str::from_utf8(value()?).map_err(|_| Errno::EINVAL)?;
                    let mode = u32::from_str_radix(mode, 8).map_err(|_| Errno::EINVAL)?;
                    options.mode = Some(mode & 0o7777);
                }
                b"uid" => options.uid = Some(id(value()?)?),
                b"gid" => options.gid = Some(id(value()?)?),
                // No transparent huge pages: only `never` is supported.
                b"huge" if value()? == b"never" => {}
                b"inode32" | b"inode64" | b"noswap" if param.value.is_none() => {}
                _ => return Err(Errno::EINVAL),
            }
        }
        Ok(options)
    }

    /// A new instance as `options` say, from the defaults: half the machine's memory and an
    /// inode per two of its pages, and a sticky world-writable root owned by the mounter
    /// (`shmem_init_fs_context`). The ids of `uid=` and `gid=` are the mounter's user
    /// namespace's, which must map them (`EINVAL`).
    fn make(&self, options: &TmpfsOptions, cred: &Cred) -> Result<Arc<MemFs>, Errno> {
        if options.quotas.types != 0 && !cred.user_ns.is_initial() {
            return Err(Errno::EINVAL);
        }
        let defaults = Limits::for_memory(Limits::DEFAULT_MEMORY);
        let limits = Limits {
            max_bytes: options.max_bytes.or(defaults.max_bytes),
            max_inodes: options.max_inodes.or(defaults.max_inodes),
        };
        let clock = Arc::new(HostClock(self.host.clone()));
        let mode = options.mode.unwrap_or(0o1777);
        let ns = &cred.user_ns;
        let uid = match options.uid {
            Some(uid) => ns.make_kuid(uid).ok_or(Errno::EINVAL)?,
            None => cred.fsuid,
        };
        let gid = match options.gid {
            Some(gid) => ns.make_kgid(gid).ok_or(Errno::EINVAL)?,
            None => cred.fsgid,
        };
        let mut fs = MemFs::with_limits_and_quotas(clock, mode, uid, gid, limits, options.quotas)?;
        if cfg!(any(target_os = "linux", target_os = "android")) {
            fs = fs.with_shared_pages(Arc::new(HostPages(self.host.clone())));
        }
        let fs = Arc::new(fs);
        self.adopt(&fs);
        Ok(fs)
    }

    /// Counts `fs`, a memory filesystem made elsewhere, as an instance, so that a remount can
    /// change its limits.
    fn adopt(&self, fs: &Arc<MemFs>) {
        let mut instances = self
            .instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        instances.retain(|instance| instance.strong_count() > 0);
        instances.push(Arc::downgrade(fs));
    }
}

impl FilesystemType for Tmpfs {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        let options = Self::options(params)?;
        let fs = self.make(&options, cred)?;
        Ok(fs)
    }

    /// tmpfs is `FS_USERNS_MOUNT`: an empty memory filesystem shows nothing of the machine.
    fn userns_mount(&self) -> bool {
        true
    }

    /// `shmem_parse_one`.
    fn parse_param(&self, param: &FsParam<'_>) -> Result<(), Errno> {
        Self::options(std::slice::from_ref(param)).map(drop)
    }

    /// `shmem_reconfigure`: the size and inode limits change; the root's mode and owner are
    /// the mount's alone.
    fn reconfigure(&self, fs: &Arc<dyn Filesystem>, params: &[FsParam<'_>]) -> Result<(), Errno> {
        let options = Self::options(params)?;
        let instances = self
            .instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let instance = instances
            .iter()
            .filter_map(Weak::upgrade)
            .find(|instance| std::ptr::addr_eq(Arc::as_ptr(instance), Arc::as_ptr(fs)));
        let Some(instance) = instance else {
            return match params.is_empty() {
                true => Ok(()),
                false => Err(Errno::EINVAL),
            };
        };
        instance.check_quota_remount(options.quotas)?;
        instance.set_limits(Limits {
            max_bytes: options.max_bytes,
            max_inodes: options.max_inodes,
        })
    }
}

/// `devtmpfs`: the one instance mounted at `/dev`, wherever it is mounted again
/// (`devtmpfs_get_tree`).
struct Devtmpfs(Arc<dyn Filesystem>);

impl FilesystemType for Devtmpfs {
    fn mount(
        &self,
        _source: &[u8],
        params: &[FsParam<'_>],
        _cred: &Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        // devtmpfs is a tmpfs: it takes tmpfs's options, which change nothing of the existing
        // instance.
        Tmpfs::options(params)?;
        Ok(self.0.clone())
    }

    fn parse_param(&self, param: &FsParam<'_>) -> Result<(), Errno> {
        Tmpfs::options(std::slice::from_ref(param)).map(drop)
    }
}

/// The page size tmpfs counts in.
const PAGE: u64 = 4096;

/// `memparse`: a number in C notation (`0x` hex, leading `0` octal) with an optional `K`, `M`,
/// `G`, `T`, `P` or `E` suffix, and what follows it.
fn memparse(text: &[u8]) -> Result<(u64, &[u8]), Errno> {
    let (radix, digits) = match text {
        [b'0', b'x' | b'X', rest @ ..] => (16, rest),
        [b'0', rest @ ..] if !rest.is_empty() => (8, rest),
        _ => (10, text),
    };
    let end = digits
        .iter()
        .position(|&b| !(b as char).is_digit(radix))
        .unwrap_or(digits.len());
    if end == 0 && radix != 8 {
        return Err(Errno::EINVAL);
    }
    let number = std::str::from_utf8(&digits[..end]).map_err(|_| Errno::EINVAL)?;
    let mut value = match number {
        "" => 0,
        number => u64::from_str_radix(number, radix).map_err(|_| Errno::EINVAL)?,
    };
    let mut rest = &digits[end..];
    let shift = match rest.first().map(u8::to_ascii_uppercase) {
        Some(b'K') => 10,
        Some(b'M') => 20,
        Some(b'G') => 30,
        Some(b'T') => 40,
        Some(b'P') => 50,
        Some(b'E') => 60,
        _ => 0,
    };
    if shift != 0 {
        value = value.checked_shl(shift).ok_or(Errno::EINVAL)?;
        rest = &rest[1..];
    }
    Ok((value, rest))
}

/// `memparse` of a whole value.
fn memparse_all(text: &[u8]) -> Result<u64, Errno> {
    match memparse(text)? {
        (value, b"") => Ok(value),
        _ => Err(Errno::EINVAL),
    }
}

/// A user or group id option (`fsparam_uid`): decimal, and not `-1`.
fn id(text: &[u8]) -> Result<u32, Errno> {
    let text = std::str::from_utf8(text).map_err(|_| Errno::EINVAL)?;
    match text.parse::<u32>() {
        Ok(u32::MAX) | Err(_) => Err(Errno::EINVAL),
        Ok(id) => Ok(id),
    }
}

/// The host's shared memory objects, for a memory filesystem to keep files in.
struct HostPages(Arc<dyn Host>);

impl Provider for HostPages {
    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
        self.0.shared_pages(size)
    }
}

/// The default root filesystem: a memory filesystem whose top directory `user` owns, laid out
/// as a small Linux system. It has empty `/bin`, `/usr/bin` and `/usr/lib` for packages and
/// mounts to fill, `/root` and the user's home `/home/user`, and in `/etc` `passwd` and `group`
/// naming root, `nobody` and the user, `hostname`, `hosts`, `protocols` and, when there are
/// name servers, `resolv.conf`. `/run` holds runtime state (`ip netns` keeps namespaces in `/run/netns`), and
/// `/var/run` links to it, as on a Debian system. `/proc`, `/dev` and `/tmp` are mount points.
///
/// `/lib/modules/<release>` describes the kernel as its package installs it, `release` being
/// what `uname -r` reports: `modules.builtin` names the drivers it has built in, the Linux
/// modules whose interfaces it implements (`veth`, `dummy`, ...), and `modules.dep` is empty,
/// since nothing is loadable. Programs that check for a driver, as LTP's
/// `tst_require_drivers` does, look there.
pub fn default_root(
    host: &Arc<dyn Host>,
    resolv_conf: &str,
    user: &Credentials,
    release: &str,
) -> Arc<dyn Filesystem> {
    let (uid, gid) = (user.uid(), user.gid());
    // mm/shmem.c shmem_mmap_prepare: root tmpfs files share their pages across fork too.
    let root: Arc<dyn Filesystem> = Arc::new(shared_tmpfs(host, 0o755, uid, gid));
    let vfs = Vfs::new(root.clone());
    let cred = Cred::root();
    let top = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &top,
        umask: 0,
        task: 0,
        leases: None,
    };
    let modules = format!("/lib/modules/{release}");
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
            "/run",
            "/var",
            &modules,
        ];
        for dir in dirs {
            vfs.mkdir_all(&cx, dir.as_bytes()).await?;
        }
        vfs.mkdir(&cx, &top, b"/root", 0o700).await?;
        vfs.symlink(&cx, b"/run", &top, b"/var/run").await?;
        let home = vfs
            .lookup(&cx, &top, b"/home/user", LookupFlags::follow())
            .await?;
        vfs.chown(&cx, &home, Some(uid), Some(gid)).await?;
        let (passwd, group) = (passwd(uid, gid), group(gid));
        let (builtin, dep) = (
            format!("{modules}/modules.builtin"),
            format!("{modules}/modules.dep"),
        );
        let mut files = vec![
            ("/etc/passwd", passwd.as_str()),
            ("/etc/group", group.as_str()),
            ("/etc/hostname", HOSTNAME),
            ("/etc/hosts", HOSTS),
            (builtin.as_str(), MODULES_BUILTIN),
            (dep.as_str(), ""),
            ("/etc/protocols", PROTOCOLS),
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

/// `modules.builtin`: the Linux modules the kernel has built in, by their paths under
/// `kernel/` as Linux's build lists them, for the interfaces it implements: dummy and veth
/// links, IPv6, packet and Unix sockets.
const MODULES_BUILTIN: &str = "kernel/drivers/net/dummy.ko\n\
                               kernel/drivers/net/veth.ko\n\
                               kernel/net/ipv6/ipv6.ko\n\
                               kernel/net/packet/af_packet.ko\n\
                               kernel/net/unix/unix.ko\n";

/// musl's `_PATH_DEFPATH`, what `execvp` searches without a `PATH`.
const DEFAULT_PATH: &str = "/usr/local/bin:/bin:/usr/bin";

/// The accounts every [`default_root`] has, as Debian's base-passwd names them: root, and
/// `nobody`, the unprivileged user that programs and test suites (LTP's root-only tests)
/// switch to, with the overflow id 65534. `(name, uid, gid, home, shell)`. Debian's other
/// system accounts are left out: the default user is uid 1, `daemon`'s there.
const SYSTEM_USERS: [(&str, u32, u32, &str, &str); 2] = [
    ("root", 0, 0, "/root", "/bin/sh"),
    ("nobody", 65534, 65534, "/nonexistent", "/usr/sbin/nologin"),
];

/// The groups every [`default_root`] has, as Debian's base-passwd names them: root's,
/// `users` for ordinary users, which programs look up by name, and `nobody`'s, `nogroup`.
const SYSTEM_GROUPS: [(&str, u32); 3] = [("root", 0), ("users", 100), ("nogroup", 65534)];

/// The home directory of `uid` in the [`default_root`]'s `/etc/passwd`.
fn home_dir(uid: u32) -> &'static str {
    match SYSTEM_USERS.iter().find(|user| user.1 == uid) {
        Some(user) => user.3,
        None => "/home/user",
    }
}

/// `/etc/passwd`: the system accounts, and the user processes run as unless it is one of them.
fn passwd(uid: u32, gid: u32) -> String {
    let mut passwd = String::new();
    for (name, uid, gid, home, shell) in SYSTEM_USERS {
        passwd += &format!("{name}:x:{uid}:{gid}:{name}:{home}:{shell}\n");
    }
    if !SYSTEM_USERS.iter().any(|user| user.1 == uid) {
        passwd += &format!("user:x:{uid}:{gid}:user:{}:/bin/sh\n", home_dir(uid));
    }
    passwd
}

/// `/etc/group`: the system groups, and the user's unless it is one of them.
fn group(gid: u32) -> String {
    let mut group = String::new();
    for (name, gid) in SYSTEM_GROUPS {
        group += &format!("{name}:x:{gid}:\n");
    }
    if !SYSTEM_GROUPS.iter().any(|group| group.1 == gid) {
        group += &format!("user:x:{gid}:\n");
    }
    group
}

/// Mounts the kernel's filesystems as a Linux system has them: `/proc`, `/sys` with the cgroup
/// hierarchy at `/sys/fs/cgroup`, `/dev` (a memory
/// filesystem with the device nodes, as devtmpfs is, and the conventional links into
/// `/proc/self/fd`), memory filesystems at `/dev/shm` for POSIX shared memory and at `/tmp`,
/// world-writable and sticky, and the pty filesystem at `/dev/pts`. The memory filesystems are
/// instances of `tmpfs`, which `mount` can make more of, and `/dev` is `devtmpfs`.
async fn mount_kernel_filesystems(kernel: &Kernel, host: &Arc<dyn Host>) {
    let vfs = kernel.vfs();
    let tmpfs_type = Arc::new(Tmpfs::new(host.clone()));
    kernel
        .register_filesystem("tmpfs", tmpfs_type.clone())
        .expect("a new kernel has no tmpfs yet");
    let adopt = |fs: MemFs| -> Arc<dyn Filesystem> {
        let fs = Arc::new(fs);
        tmpfs_type.adopt(&fs);
        fs
    };
    // mm/shmem.c shmem_mmap_prepare: every tmpfs mount supplies shared pages, including /tmp.
    let tmpfs = |mode| adopt(shared_tmpfs(host, mode, 0, 0));
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
        leases: None,
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
                let text = format!("cannot mount a kernel filesystem at {at}: {errno:?}");
                kernel.printk(wasixv2_kernel::LogLevel::Warning, &text);
                false
            }
        }
    };
    let mounts = async {
        mount("/proc", kernel.procfs(), flags(true, true)).await;
        if mount("/sys", kernel.sysfs(), flags(true, true)).await {
            mount(
                "/sys/kernel/security",
                kernel.securityfs(),
                flags(true, true),
            )
            .await;
            // Where systemd and container runtimes look for the cgroup v2 hierarchy.
            mount("/sys/fs/cgroup", kernel.cgroupfs(), flags(true, true)).await;
        }
        let dev = tmpfs(0o755);
        kernel
            .register_filesystem("devtmpfs", Arc::new(Devtmpfs(dev.clone())))
            .expect("a new kernel has no devtmpfs yet");
        if mount("/dev", dev, flags(false, false)).await {
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
            let shm = adopt(shared_tmpfs(host, 0o1777, 0, 0));
            mount("/dev/shm", shm, flags(true, false)).await;
            mount("/dev/pts", kernel.devpts(), flags(false, true)).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn params(options: &[(&'static str, Option<&'static str>)]) -> Vec<FsParam<'static>> {
        options
            .iter()
            .map(|&(key, value)| FsParam {
                key: key.as_bytes(),
                value: value.map(str::as_bytes),
            })
            .collect()
    }

    fn tmpfs() -> Tmpfs {
        Tmpfs::new(Arc::new(TokioHost::default()))
    }

    /// Linux mm/shmem.c shmem_mmap_prepare: the boot-time tmpfs mounts use the same shared
    /// backing as a guest-mounted tmpfs, so stores and futexes cross fork.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[tokio::test]
    async fn default_tmpfs_files_have_shared_pages() {
        let machine = Machine::builder().user(Credentials::new(42, 43)).build();
        let vfs = machine.vfs();
        let root = vfs.root();
        let attr = root.dentry.node().getattr().await.unwrap();
        assert_eq!((attr.mode & 0o7777, attr.uid, attr.gid), (0o755, 42, 43));
        let cred = Cred::root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0,
            task: 0,
            leases: None,
        };
        let how = wasixv2_vfs::OpenHow {
            flags: wasixv2_abi::OpenFlags::O_CREAT | wasixv2_abi::OpenFlags::O_RDWR,
            mode: 0o600,
            lookup: LookupFlags::follow(),
        };
        for name in [
            b"/shared".as_slice(),
            b"/tmp/shared".as_slice(),
            b"/dev/shared",
            b"/dev/shm/shared",
        ] {
            let file = vfs.open(&cx, &root, name, &how).await.unwrap();
            let pages = file
                .path
                .dentry
                .node()
                .shared_pages(0..65536, &cred)
                .unwrap();
            assert!(pages.is_some(), "{}", String::from_utf8_lossy(name));
        }
    }

    /// The bytes `fs` may hold.
    fn size(fs: &Arc<dyn Filesystem>) -> u64 {
        now(fs.statfs()).unwrap().blocks * PAGE
    }

    #[test]
    fn memparse_takes_c_numbers_and_size_suffixes() {
        assert_eq!(memparse(b"10"), Ok((10, b"".as_slice())));
        assert_eq!(memparse(b"0x10k"), Ok((16 << 10, b"".as_slice())));
        assert_eq!(memparse(b"010"), Ok((8, b"".as_slice())));
        assert_eq!(memparse(b"2G%"), Ok((2 << 30, b"%".as_slice())));
        assert_eq!(memparse(b"0"), Ok((0, b"".as_slice())));
        assert_eq!(memparse(b"k"), Err(Errno::EINVAL));
        assert_eq!(memparse_all(b"1m"), Ok(1 << 20));
        assert_eq!(memparse_all(b"1mb"), Err(Errno::EINVAL));
    }

    #[test]
    fn tmpfs_options_are_shmems() {
        let cred = Cred::user(5, 6);
        let defaults: Arc<dyn Filesystem> =
            tmpfs().make(&Tmpfs::options(&[]).unwrap(), &cred).unwrap();
        let root = now(defaults.root().getattr()).unwrap();
        assert_eq!((root.mode & 0o7777, root.uid, root.gid), (0o1777, 5, 6));
        assert_eq!(
            Some(size(&defaults)),
            Limits::default().max_bytes,
            "half the machine's memory"
        );

        let set = params(&[
            ("size", Some("10001")),
            ("nr_inodes", Some("1k")),
            ("mode", Some("01755")),
            ("uid", Some("7")),
            ("gid", Some("8")),
            ("huge", Some("never")),
            ("inode64", None),
        ]);
        let options = Tmpfs::options(&set).unwrap();
        assert_eq!(options.max_bytes, Some(3 * PAGE), "whole pages");
        assert_eq!(options.max_inodes, Some(1024));
        assert_eq!(
            (options.mode, options.uid, options.gid),
            (Some(0o1755), Some(7), Some(8))
        );
        let half = Tmpfs::options(&params(&[("size", Some("50%"))])).unwrap();
        assert_eq!(half.max_bytes, Some(Limits::DEFAULT_MEMORY / 2));

        for bad in [
            ("size", Some("1x")),
            ("size", None),
            ("mode", Some("9")),
            ("uid", Some("-1")),
            ("huge", Some("always")),
            ("mpol", Some("default")),
            ("inode64", Some("1")),
            ("bogus", None),
        ] {
            assert_eq!(
                Tmpfs::options(&params(&[bad])).err(),
                Some(Errno::EINVAL),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn tmpfs_quota_options_and_remount_restrictions() {
        let tmpfs = tmpfs();
        let set = params(&[
            ("usrquota", None),
            ("grpquota", None),
            ("usrquota_block_hardlimit", Some("8k")),
            ("grpquota_inode_hardlimit", Some("3")),
        ]);
        let options = Tmpfs::options(&set).unwrap();
        assert_eq!(options.quotas.types, 3);
        assert_eq!(options.quotas.block_hard, [8192, 0]);
        let fs: Arc<dyn Filesystem> = tmpfs.make(&options, &Cred::root()).unwrap();
        assert!(fs.quota().unwrap().active(0));
        assert!(fs.quota().unwrap().active(1));
        assert_eq!(tmpfs.reconfigure(&fs, &set), Ok(()));
        assert_eq!(
            tmpfs.reconfigure(&fs, &params(&[("usrquota_block_hardlimit", Some("16k"))])),
            Err(Errno::EINVAL)
        );
        let plain: Arc<dyn Filesystem> =
            tmpfs.make(&TmpfsOptions::default(), &Cred::root()).unwrap();
        assert_eq!(
            tmpfs.reconfigure(&plain, &params(&[("quota", None)])),
            Err(Errno::EINVAL)
        );
        for bad in [
            ("quota", Some("1")),
            ("usrquota_block_hardlimit", Some("0")),
            ("grpquota_inode_hardlimit", Some("9223372036854775808")),
        ] {
            assert_eq!(Tmpfs::options(&params(&[bad])).err(), Some(Errno::EINVAL));
        }
    }

    #[test]
    fn a_tmpfs_remount_changes_the_limits_of_that_instance() {
        let tmpfs = tmpfs();
        let cred = Cred::root();
        let options = Tmpfs::options(&params(&[("size", Some("1m"))])).unwrap();
        let fs: Arc<dyn Filesystem> = tmpfs.make(&options, &cred).unwrap();
        let other: Arc<dyn Filesystem> = tmpfs.make(&options, &cred).unwrap();
        let resize = params(&[("size", Some("2m")), ("mode", Some("0700"))]);
        assert_eq!(tmpfs.reconfigure(&fs, &resize), Ok(()));
        assert_eq!(size(&fs), 2 << 20);
        assert_eq!(size(&other), 1 << 20);
        let root = now(fs.root().getattr()).unwrap();
        assert_eq!(root.mode & 0o7777, 0o1777, "the mode is the mount's");

        let host: Arc<dyn Host> = Arc::new(TokioHost::default());
        let unknown: Arc<dyn Filesystem> =
            Arc::new(MemFs::new(Arc::new(HostClock(host)), 0o755, 0, 0));
        assert_eq!(tmpfs.reconfigure(&unknown, &[]), Ok(()));
        assert_eq!(tmpfs.reconfigure(&unknown, &resize), Err(Errno::EINVAL));
    }
}
