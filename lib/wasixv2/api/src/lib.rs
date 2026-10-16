//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::{
    future::Future,
    io,
    path::Path,
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

pub use wasixv2_exec::ExecError;
#[cfg(not(target_family = "wasm"))]
pub use wasixv2_host::BlockingSource;
use wasixv2_exec::{Abis, Wasixv2};
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
use wasixv2_vfs::{Cred, Ctx, Errno, LookupFlags, MountFlags, Vfs};
use wasixv2_vfs_mem::MemFs;
use wasixv2_vfs_overlay::OverlayFs;
use wasixv2_vfs_package::PackageFs;
pub use wasixv2_vnet::{Net, NetConfig};
#[cfg(feature = "wasix1")]
pub use wasixv2_wasix1::{V1Flavor, is_wasix1_module};
use wasmer::Store;
use webc::v3::read::OwnedReader;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("invalid module: {0}")]
    Compile(#[from] wasmer::CompileError),
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error("cannot use the working directory: {0}")]
    WorkingDir(io::Error),
}

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and laid out as a small Linux system (see
/// [`default_root`]); [`Machine::with_root`] takes another. Either way the kernel's own
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
    network: Arc<Net>,
    host: Arc<dyn Host>,
    /// The uid and gid processes start as and filesystems are owned by.
    owner: (u32, u32),
    /// Where new processes start; `/` if unset.
    working_dir: Option<String>,
    /// The processes [`Machine::run_process`] started that are still running.
    running: Arc<Mutex<Vec<TaskId>>>,
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
}

impl MachineBuilder {
    /// A machine whose clocks, timers, entropy and executor come from `host`.
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self {
            host,
            user: Credentials::default(),
            uname: Uname::default(),
            root: None,
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
    /// a mount point that cannot be made (on a read-only root) is left out.
    pub fn root(mut self, root: Arc<dyn Filesystem>) -> Self {
        self.root = Some(root);
        self
    }

    pub fn build(self) -> Machine {
        let Self {
            host,
            user,
            uname,
            root,
        } = self;
        let owner = (user.uid(), user.gid());
        let network = default_network();
        let root = root.unwrap_or_else(|| default_root(&host, &network.resolv_conf(), &user));
        let kernel = Kernel::new(host.clone(), root)
            .with_network(network.clone())
            .with_shm(shm_filesystem(&host))
            .with_credentials(user)
            .with_uname(&uname);
        mount_kernel_filesystems(&kernel, &host);
        Machine {
            syscalls: Arc::new(Layered::new(Trace, kernel)),
            abis: Arc::new(default_abis()),
            network,
            host,
            owner,
            working_dir: None,
            running: Arc::default(),
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

    /// Sends the signal `sig` to every process started with [`Machine::run`] or
    /// [`Machine::run_process`] that is still running; see [`Signaller::signal`].
    pub fn signal(&self, sig: i32) -> io::Result<()> {
        self.signaller().signal(sig)
    }

    /// A handle that signals this machine's processes from any thread, for an embedder that
    /// forwards the signals of its host process.
    pub fn signaller(&self) -> Signaller {
        Signaller {
            syscalls: self.syscalls.clone(),
            running: self.running.clone(),
        }
    }

    /// Starts `wasm` as a new process and waits for it to exit. Returns its exit status: that of
    /// `exit_group`, or 128 + the signal that killed it, a trap of the guest included.
    pub async fn run_process(&self, wasm: &[u8], config: ProcessConfig) -> Result<i32, RunError> {
        let store = Store::default();
        let module = wasixv2_exec::load(store.engine(), wasm)?;
        let task = self.syscalls.inner.spawn(config);
        self.running.lock().unwrap().push(task);
        let _running = Running {
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
        let host = self.host.clone();
        let (syscalls, abis) = (self.syscalls.clone(), self.abis.clone());
        Ok(wasixv2_exec::run_with_abis(store, &module, syscalls, task, host, abis).await?)
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
    /// Sends the signal `sig` to every process started with [`Machine::run`] or
    /// [`Machine::run_process`] that is still running, as a signal from outside the machine
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

fn default_abis() -> Abis<Layered<Trace, Kernel>> {
    let mut abis = Abis::new();
    abis.push(Arc::new(Wasixv2));
    #[cfg(feature = "wasix1")]
    abis.push(Arc::new(wasixv2_wasix1::Wasix1::new()));
    abis
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
fn mount_kernel_filesystems(kernel: &Kernel, host: &Arc<dyn Host>) {
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
    now(mounts).expect("the kernel's filesystems take their layout");
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

/// Whether `wasm` targets this ABI: it imports something from the `wasixv2` module. Only the
/// import section is parsed; a malformed module is `false`.
pub fn is_wasixv2_module(wasm: &[u8]) -> bool {
    use wasmparser::{Parser, Payload};

    for payload in Parser::new(0).parse_all(wasm) {
        match payload {
            Ok(Payload::ImportSection(imports)) => {
                return imports
                    .into_imports()
                    .any(|import| import.is_ok_and(|i| i.module == wasixv2_exec::IMPORT_MODULE));
            }
            // Sections are ordered, so the imports are behind us once code-related ones start.
            Ok(Payload::FunctionSection(_) | Payload::CodeSectionStart { .. }) | Err(_) => {
                return false;
            }
            Ok(_) => {}
        }
    }
    false
}
