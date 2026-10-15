//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::sync::Arc;

pub use wasixv2_exec::ExecError;
use wasixv2_host::TokioHost;
pub use wasixv2_host::{ByteSink, ByteSource, LocalFuture};
use wasixv2_kernel::Kernel;
pub use wasixv2_kernel::{ProcessConfig, Stdio};
use wasixv2_syscalls::Layered;
use wasixv2_trace::Trace;
use wasixv2_vfs::{FileType, Inode};
use wasixv2_vfs_mem::MemDir;
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
/// Must be driven by a tokio runtime with timers enabled. The root filesystem is in memory and
/// starts out with an empty `/tmp`.
pub struct Machine {
    syscalls: Arc<Layered<Trace, Kernel>>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        let kernel = Kernel::new(Arc::new(TokioHost::default()), root_filesystem());
        Self {
            syscalls: Arc::new(Layered::new(Trace, kernel)),
        }
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

fn root_filesystem() -> Arc<dyn Inode> {
    let root = MemDir::root();
    root.create(b"tmp", FileType::Directory)
        .expect("a new directory is empty");
    root
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
