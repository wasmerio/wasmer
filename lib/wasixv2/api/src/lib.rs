//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::sync::Arc;

pub use wasixv2_exec::ExecError;
#[cfg(feature = "tokio")]
pub use wasixv2_host::TokioHost;
#[cfg(feature = "web")]
pub use wasixv2_host::WebHost;
pub use wasixv2_host::{ByteSink, ByteSource, EmptySource, Executor, FnSink, Host, LocalFuture};
use wasixv2_kernel::Kernel;
pub use wasixv2_kernel::{ProcessConfig, Stdio};
use wasixv2_syscalls::Layered;
use wasixv2_trace::Trace;
use wasixv2_vfs::{FileType, Inode};
use wasixv2_vfs_mem::MemDir;
pub use wasixv2_vnet::{Net, NetConfig};
use wasmer::{Module, Store};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("invalid module: {0}")]
    Compile(#[from] wasmer::CompileError),
    #[error(transparent)]
    Exec(#[from] ExecError),
}

/// One kernel and the processes it runs.
///
/// The root filesystem is in memory and starts out with an empty `/tmp` and, when there are
/// name servers, `/etc/resolv.conf`. The network is one namespace; with the `host-network`
/// feature on a Unix host, traffic leaving it goes through host sockets, otherwise it is
/// loopback-only. The machine's futures must be driven where its [`Host`]'s executor expects
/// them; see [`Machine::new`].
pub struct Machine {
    syscalls: Arc<Layered<Trace, Kernel>>,
    network: Arc<Net>,
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
        let kernel = Kernel::new(host, root_filesystem(&network.resolv_conf()))
            .with_network(network.clone());
        Self {
            syscalls: Arc::new(Layered::new(Trace, kernel)),
            network,
        }
    }

    /// The machine's network namespace, e.g. to [`Net::publish`] a host port to a guest
    /// listener.
    pub fn network(&self) -> &Arc<Net> {
        &self.network
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

    /// Starts `wasm` as a new process and waits for it to exit. Returns its exit status.
    pub async fn run_process(&self, wasm: &[u8], config: ProcessConfig) -> Result<i32, RunError> {
        let task = self.syscalls.inner.spawn(config);
        let store = Store::default();
        let module = Module::new(&store, wasm)?;
        Ok(wasixv2_exec::run(store, &module, self.syscalls.clone(), task).await?)
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

fn root_filesystem(resolv_conf: &str) -> Arc<dyn Inode> {
    let root = MemDir::root();
    root.create(b"tmp", FileType::Directory)
        .expect("a new directory is empty");
    if !resolv_conf.is_empty() {
        let etc = root
            .create(b"etc", FileType::Directory)
            .expect("a new directory is empty");
        let file = etc
            .create(b"resolv.conf", FileType::Regular)
            .expect("a new directory is empty");
        write_now(&*file, resolv_conf.as_bytes());
    }
    root
}

/// Writes to an in-memory file, whose futures are ready at once.
fn write_now(file: &dyn Inode, data: &[u8]) {
    use std::task::{Context, Poll, Waker};

    let mut write = file.write_at(data, 0);
    let poll = write.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(
        matches!(poll, Poll::Ready(Ok(n)) if n == data.len()),
        "in-memory writes complete at once"
    );
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
