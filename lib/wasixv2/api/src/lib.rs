//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::{
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

pub use wasixv2_exec::ExecError;
use wasixv2_exec::{Abis, Wasixv2};
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
use wasixv2_vfs_overlay::{MAX_LOWERS, OverlayFs};
use wasixv2_vfs_package::PackageFs;
pub use wasixv2_vnet::{Net, NetConfig};
#[cfg(feature = "wasix1")]
pub use wasixv2_wasix1::{V1Flavor, is_wasix1_module, module_flavor as wasix1_module_flavor};
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
}

/// The uid and gid processes run as, which own the root filesystem.
const USER: u32 = 1;

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and starts out with an empty `/tmp`, `/etc/hosts` and, when there are
/// name servers, `/etc/resolv.conf`; [`Machine::mount_root_lower`] stacks package volumes under
/// it. The network is one namespace; with the `host-network`
/// feature on a Unix host, traffic leaving it goes through host sockets, otherwise it is
/// loopback-only. The machine's futures must be driven where its [`Host`]'s executor expects
/// them; see [`Machine::new`].
pub struct Machine {
    syscalls: Arc<Layered<Trace, Kernel>>,
    /// The guest ABIs programs may target: `wasixv2`, then WASIX v1 with the `wasix1` feature.
    abis: Arc<Abis<Layered<Trace, Kernel>>>,
    network: Arc<Net>,
    host: Arc<dyn Host>,
    /// Where new processes start; `/` if unset.
    working_dir: Option<String>,
    /// The processes [`Machine::run_process`] started that are still running.
    running: Mutex<Vec<TaskId>>,
    /// The root filesystem's writable top layer, which holds `/tmp` and `/etc`.
    root_upper: Arc<dyn Filesystem>,
    /// Read-only layers under `root_upper`, topmost first; see [`Machine::mount_root_lower`].
    root_lowers: Vec<Arc<dyn Filesystem>>,
    /// Set by the first mount or process: from then on the root filesystem is fixed.
    in_use: AtomicBool,
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

    /// A machine whose clocks, timers, entropy and executor come from `host`.
    pub fn with_host(host: Arc<dyn Host>) -> Self {
        let network = default_network();
        let root = root_filesystem(&host, &network.resolv_conf());
        Self {
            syscalls: kernel(&host, root.clone(), &network),
            abis: Arc::new(default_abis()),
            network,
            host,
            working_dir: None,
            running: Mutex::default(),
            root_upper: root,
            root_lowers: Vec::new(),
            in_use: AtomicBool::new(false),
        }
    }

    /// Makes the volume `volume` of the webc v3 package `reader` a read-only layer of the root
    /// filesystem, under the in-memory layer that holds `/tmp`, `/etc/hosts`, `/etc/resolv.conf`
    /// and every change, and over the layers earlier calls added. Objects in the volume appear
    /// owned by the guest user. This is how a package's volume at `/` is mounted.
    ///
    /// The root can only change on a fresh machine: this fails with `EBUSY` after anything was
    /// mounted or run.
    pub async fn mount_root_lower(&mut self, reader: &OwnedReader, volume: &str) -> io::Result<()> {
        let lower = PackageFs::with_owner(reader, volume, USER, USER).map_err(io::Error::other)?;
        self.push_root_lower(Arc::new(lower)).await
    }

    /// [`Machine::mount_root_lower`] for the host directory `host_dir`, which the guest sees
    /// through the root but never changes: changes stay in the in-memory layer. This is how a
    /// host directory mapped to `/` is mounted, since nothing can be mounted over the root.
    pub async fn mount_root_lower_host_dir(&mut self, host_dir: &Path) -> io::Result<()> {
        let options = wasixv2_vfs_host::Options {
            read_only: true,
            owner: (USER, USER),
        };
        let lower = wasixv2_vfs_host::open(host_dir, self.host.clone(), options)?;
        self.push_root_lower(lower).await
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
        // Nothing lives in the kernel yet, so a new one on the new root replaces it.
        self.syscalls = kernel(&self.host, root, &self.network);
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
        let engine = Engine::default();
        let module = Module::new(&engine, wasm)?;
        self.run_module(&engine, &module, config).await
    }

    /// Starts `module`, compiled by `engine`, as a new process and waits for it to exit. Returns
    /// its exit status. Threads and the images the process `execve`s are compiled by `engine`
    /// too, so an embedder picks the compiler and can cache compiled modules.
    pub async fn run_module(
        &self,
        engine: &Engine,
        module: &Module,
        config: ProcessConfig,
    ) -> Result<i32, RunError> {
        self.in_use.store(true, Ordering::Relaxed);
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
        let store = Store::new(engine.clone());
        let host = self.host.clone();
        let (syscalls, abis) = (self.syscalls.clone(), self.abis.clone());
        Ok(wasixv2_exec::run_with_abis(store, module, syscalls, task, host, abis).await?)
    }
}

fn kernel(
    host: &Arc<dyn Host>,
    root: Arc<dyn Filesystem>,
    network: &Arc<Net>,
) -> Arc<Layered<Trace, Kernel>> {
    let kernel = Kernel::new(host.clone(), root).with_network(network.clone());
    Arc::new(Layered::new(Trace, kernel))
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

/// The guest's `/etc/hosts`.
const HOSTS: &str = "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\n";

/// A memory root owned by the user processes run as, with a world-writable sticky `/tmp`,
/// `/etc/hosts` and, when there are name servers, `/etc/resolv.conf`.
fn root_filesystem(host: &Arc<dyn Host>, resolv_conf: &str) -> Arc<dyn Filesystem> {
    let root: Arc<dyn Filesystem> = Arc::new(MemFs::new(
        Arc::new(HostClock(host.clone())),
        0o755,
        USER,
        USER,
    ));
    let vfs = Vfs::new(root.clone());
    let cred = Cred::user(USER, USER);
    let top = vfs.root();
    let cx = Ctx {
        cred: &cred,
        root: &top,
        umask: 0,
    };
    let tmp = async {
        vfs.mkdir(&cx, &top, b"/tmp", 0o1777).await?;
        let path = vfs
            .lookup(&cx, &top, b"/tmp", LookupFlags::follow())
            .await?;
        vfs.chmod(&cx, &path, 0o1777).await?;
        vfs.mkdir(&cx, &top, b"/etc", 0o755).await?;
        let how = wasixv2_vfs::OpenHow {
            flags: wasixv2_abi::OpenFlags::O_CREAT | wasixv2_abi::OpenFlags::O_WRONLY,
            mode: 0o644,
            lookup: LookupFlags::follow(),
        };
        // `localhost` resolves without DNS, as on every Linux system.
        let file = vfs.open(&cx, &top, b"/etc/hosts", &how).await?;
        file.write_at(HOSTS.as_bytes(), 0).await?;
        if !resolv_conf.is_empty() {
            let file = vfs.open(&cx, &top, b"/etc/resolv.conf", &how).await?;
            file.write_at(resolv_conf.as_bytes(), 0).await?;
        }
        Ok::<_, Errno>(())
    };
    // A memory filesystem never waits.
    match pin!(tmp).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result.expect("a new memory filesystem accepts /tmp and /etc"),
        Poll::Pending => unreachable!("memory filesystem operations complete at once"),
    }
    root
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
