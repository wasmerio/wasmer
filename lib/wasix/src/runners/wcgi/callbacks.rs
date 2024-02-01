use std::{collections::HashMap, sync::Arc};

use virtual_fs::Pipe;
use wasmer::{Memory, Module, Store};

use crate::{runtime::module_cache::ModuleHash, WasiEnv};

use super::{create_env::default_recycle_env, handler::SetupBuilder, *};

/// Configuration used for creating a new environment
pub struct CreateEnvConfig {
    pub env: HashMap<String, String>,
    pub program_name: String,
    pub module: Module,
    pub module_hash: ModuleHash,
    pub runtime: Arc<dyn crate::runtime::Runtime + Send + Sync>,
    pub setup_builder: SetupBuilder,
}

/// Result of a create operation on a new environment
pub struct CreateEnvResult {
    pub env: WasiEnv,
    pub memory: Option<(Memory, Store)>,
    pub body_sender: Pipe,
    pub body_receiver: Pipe,
    pub stderr_receiver: Pipe,
}

/// Configuration used for reusing an new environment
pub struct RecycleEnvConfig {
    pub env: WasiEnv,
    pub memory: Memory,
    pub store: Store,
}

/// Callbacks that are triggered at various points in the lifecycle of a runner
/// and any WebAssembly instances it may start.
#[async_trait::async_trait]
pub trait Callbacks: Send + Sync + 'static {
    /// A callback that is called whenever the server starts.
    fn started(&self, _abort: AbortHandle) {}

    /// Data was written to stderr by an instance.
    fn on_stderr(&self, _stderr: &[u8]) {}

    /// Reading from stderr failed.
    fn on_stderr_error(&self, _error: std::io::Error) {}

    /// Recycle the WASI environment
    async fn recycle_env(&self, conf: RecycleEnvConfig) {
        default_recycle_env(conf).await
    }

    /// Create the WASI environment
    async fn create_env(&self, conf: CreateEnvConfig) -> anyhow::Result<CreateEnvResult> {
        default_create_env(conf).await
    }
}

pub struct NoOpWcgiCallbacks;

impl Callbacks for NoOpWcgiCallbacks {}
