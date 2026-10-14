//! Embedding facade: assembles the kernel, filesystem drivers, network backends, layers and the
//! execution engine into a machine, and boots it.

use std::sync::Arc;

pub use wasixv2_exec::ExecError;
pub use wasixv2_host::{ByteSink, ByteSource, LocalFuture};
use wasixv2_kernel::Kernel;
pub use wasixv2_kernel::Stdio;
use wasixv2_syscalls::Layered;
use wasixv2_trace::Trace;
use wasmer::{Module, Store};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("invalid module: {0}")]
    Compile(#[from] wasmer::CompileError),
    #[error(transparent)]
    Exec(#[from] ExecError),
}

/// One kernel and the processes it runs.
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
        Self {
            syscalls: Arc::new(Layered::new(Trace, Kernel::new())),
        }
    }

    /// Starts `wasm` as a new process and waits for it to exit. Returns its exit status.
    pub async fn run(&self, wasm: &[u8], stdio: Stdio) -> Result<i32, RunError> {
        let task = self.syscalls.inner.spawn(stdio);
        let store = Store::default();
        let module = Module::new(&store, wasm)?;
        Ok(wasixv2_exec::run(store, &module, self.syscalls.clone(), task).await?)
    }
}
