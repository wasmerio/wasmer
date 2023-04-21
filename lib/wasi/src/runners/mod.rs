mod runner;

#[cfg(feature = "webc_runner_rt_emscripten")]
pub mod emscripten;
#[cfg(feature = "webc_runner_rt_wasi")]
pub mod wasi;
#[cfg(any(feature = "webc_runner_rt_wasi", feature = "webc_runner_rt_wcgi"))]
mod wasi_common;
#[cfg(feature = "webc_runner_rt_wcgi")]
pub mod wcgi;

pub use self::runner::Runner;

use anyhow::Error;
use wasmer::{Engine, Module};

pub type CompileModule = dyn Fn(&Engine, &[u8]) -> Result<Module, Error> + Send + Sync;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    pub host: std::path::PathBuf,
    pub guest: String,
}

pub(crate) fn default_compile(engine: &Engine, wasm: &[u8]) -> Result<Module, Error> {
    let module = Module::new(engine, wasm)?;
    Ok(module)
}
