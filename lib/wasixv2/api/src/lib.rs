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
#[cfg(feature = "tokio")]
pub use wasixv2_host::TokioHost;
#[cfg(feature = "web")]
pub use wasixv2_host::WebHost;
pub use wasixv2_host::{
    ByteSink, ByteSource, EmptySource, Executor, FnSink, Host, LocalFuture, ThreadStart,
};
use wasixv2_kernel::{HostClock, Kernel};
pub use wasixv2_kernel::{ProcessConfig, Stdio};
use wasixv2_syscalls::{Layered, TaskId};
use wasixv2_trace::Trace;
pub use wasixv2_vfs::Filesystem;
use wasixv2_vfs::{Cred, Ctx, Errno, LookupFlags, MountFlags, Vfs};
use wasixv2_vfs_mem::MemFs;
use wasixv2_vfs_overlay::OverlayFs;
use wasixv2_vfs_package::PackageFs;
pub use wasixv2_vnet::{Net, NetConfig};
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
}

/// The uid and gid processes run as, which own the root filesystem.
const USER: u32 = 1;

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and laid out as a small Linux system (see
/// [`default_root`]); [`Machine::with_root`] takes another. Either way the kernel's own
/// filesystems are mounted on it: `/proc`, `/dev` with the device nodes, and memory
/// filesystems at `/dev/shm` and `/tmp`. The network is one namespace; with the `host-network`
/// feature on a Unix host, traffic leaving it goes through host sockets, otherwise it is
/// loopback-only. The machine's futures must be driven where its [`Host`]'s executor expects
/// them; see [`Machine::new`].
pub struct Machine {
    syscalls: Arc<Layered<Trace, Kernel>>,
    network: Arc<Net>,
    host: Arc<dyn Host>,
    /// Where new processes start; `/` if unset.
    working_dir: Option<String>,
    /// The processes [`Machine::run_process`] started that are still running.
    running: Mutex<Vec<TaskId>>,
}

#[cfg(feature = "tokio")]
impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    /// A machine on [`TokioHost`]. Drive it on a tokio runtime with timers enabled, inside a
    /// `tokio::task::LocalSet`, which background host work is spawned onto.
    #[cfg(feature = "tokio")]
    pub fn new() -> Self {
        Self::with_host(Arc::new(TokioHost::default()))
    }

    /// A machine whose clocks, timers, entropy and executor come from `host`, on the
    /// [`default_root`].
    pub fn with_host(host: Arc<dyn Host>) -> Self {
        let network = default_network();
        let root = default_root(&host, &network.resolv_conf());
        Self::assemble(host, root, network)
    }

    /// A machine on `root` instead of the default root filesystem, such as an embedder's
    /// image. `/proc`, `/dev`, `/dev/shm` and `/tmp` are mounted on it, their directories made
    /// where missing; a mount point that cannot be made (on a read-only root) is left out.
    pub fn with_root(host: Arc<dyn Host>, root: Arc<dyn Filesystem>) -> Self {
        Self::assemble(host, root, default_network())
    }

    fn assemble(host: Arc<dyn Host>, root: Arc<dyn Filesystem>, network: Arc<Net>) -> Self {
        let kernel = Kernel::new(host.clone(), root).with_network(network.clone());
        mount_kernel_filesystems(&kernel, &host);
        Self {
            syscalls: Arc::new(Layered::new(Trace, kernel)),
            network,
            host,
            working_dir: None,
            running: Mutex::default(),
        }
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
    /// `host_dir`; files the host user owns appear as owned by the guest user.
    pub async fn mount_host_dir(
        &self,
        guest: &str,
        host_dir: &Path,
        read_only: bool,
    ) -> io::Result<()> {
        let options = wasixv2_vfs_host::Options {
            read_only,
            owner: (USER, USER),
        };
        let fs = wasixv2_vfs_host::open(host_dir, self.host.clone(), options)?;
        self.mount(guest, fs, read_only).await
    }

    /// Mounts the volume `volume` of the webc v3 package `reader` at `guest`, sharing the
    /// reader's buffer. With `writable`, the volume is the lower layer of an overlay whose upper
    /// layer is in memory, so changes work and are lost with the machine; otherwise the mount is
    /// read-only. Objects in the volume appear owned by the guest user.
    pub async fn mount_package_volume(
        &self,
        guest: &str,
        reader: &OwnedReader,
        volume: &str,
        writable: bool,
    ) -> io::Result<()> {
        let lower: Arc<dyn Filesystem> =
            Arc::new(PackageFs::with_owner(reader, volume, USER, USER).map_err(io::Error::other)?);
        let fs = if writable {
            let upper: Arc<dyn Filesystem> = Arc::new(MemFs::new(
                Arc::new(HostClock(self.host.clone())),
                0o755,
                USER,
                USER,
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
    /// [`Machine::run_process`] that is still running, as a signal from outside the machine
    /// (`SI_USER` with `si_pid` 0). It goes through the kernel's normal signal generation:
    /// the guest's handler runs if it installed one, and otherwise the default action does, so
    /// e.g. an unhandled `SIGTERM` ends the process with status 128 + 15.
    ///
    /// A task notices a pending signal only at an interruption point, a blocking call or the
    /// return from a syscall; a guest spinning without syscalls does not see it.
    ///
    /// Fails with `EINVAL` for a signal number outside 1..=64.
    pub fn signal(&self, sig: i32) -> io::Result<()> {
        if !(1..=64).contains(&sig) {
            return Err(errno_to_io(Errno::EINVAL));
        }
        let running = self.running.lock().unwrap().clone();
        for task in running {
            match self.syscalls.inner.kill(task, wasixv2_abi::Signal(sig)) {
                // It exited meanwhile.
                Ok(()) | Err(Errno::ESRCH) => {}
                Err(errno) => return Err(errno_to_io(errno)),
            }
        }
        Ok(())
    }

    /// Starts `wasm` as a new process and waits for it to exit. Returns its exit status.
    pub async fn run_process(&self, wasm: &[u8], config: ProcessConfig) -> Result<i32, RunError> {
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
        let store = Store::default();
        let module = Module::new(&store, wasm)?;
        let host = self.host.clone();
        Ok(wasixv2_exec::run(store, &module, self.syscalls.clone(), task, host).await?)
    }
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
/// The users and groups: root, and the user processes run as.
const PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\nuser:x:1:1:user:/home/user:/bin/sh\n";
const GROUP: &str = "root:x:0:\nuser:x:1:\n";
/// The kernel's nodename.
const HOSTNAME: &str = "localhost\n";

/// The default root filesystem: a memory filesystem whose top directory the user processes run
/// as owns, laid out as a small Linux system. It has empty `/bin`, `/usr/bin` and `/usr/lib`
/// for packages and mounts to fill, `/root` and the user's home `/home/user`, and in `/etc`
/// `passwd` and `group` naming both users, `hostname`, `hosts` and, when there are name
/// servers, `resolv.conf`. `/proc`, `/dev` and `/tmp` are mount points.
pub fn default_root(host: &Arc<dyn Host>, resolv_conf: &str) -> Arc<dyn Filesystem> {
    let root: Arc<dyn Filesystem> = Arc::new(MemFs::new(
        Arc::new(HostClock(host.clone())),
        0o755,
        USER,
        USER,
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
        vfs.chown(&cx, &home, Some(USER), Some(USER)).await?;
        let mut files = vec![
            ("/etc/passwd", PASSWD),
            ("/etc/group", GROUP),
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
