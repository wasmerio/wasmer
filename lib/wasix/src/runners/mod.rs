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

use anyhow::{Context, Error};
use wasmer::Module;

use crate::runtime::{
    module_cache::{CacheError, ModuleHash},
    Runtime,
};

/// A directory that should be mapped from the host filesystem into a WASI
/// instance (the "guest").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MappedDirectory {
    /// The absolute path for a directory on the host filesystem.
    pub host: std::path::PathBuf,
    /// The absolute path specifying where the host directory should be mounted
    /// inside the guest.
    pub guest: String,
}

/// Compile a module, trying to use a pre-compiled version if possible.
#[cfg(any(
    feature = "webc_runner_rt_wasi",
    feature = "webc_runner_rt_wcgi",
    feature = "webc_runner_rt_emscripten",
))]
pub(crate) fn compile_module(wasm: &[u8], runtime: &dyn Runtime) -> Result<Module, Error> {
    // TODO(Michael-F-Bryan,theduke): This should be abstracted out into some
    // sort of ModuleResolver component that is attached to the runtime and
    // encapsulates finding a WebAssembly binary, compiling it, and caching.

    let engine = runtime.engine().context("No engine provided")?;
    let task_manager = runtime.task_manager().clone();
    let module_cache = runtime.module_cache();

    let hash = ModuleHash::sha256(wasm);
    let result = task_manager.block_on(module_cache.load(hash, &engine));

    match result {
        Ok(module) => return Ok(module),
        Err(CacheError::NotFound) => {}
        Err(other) => {
            tracing::warn!(
                %hash,
                error=&other as &dyn std::error::Error,
                "Unable to load the cached module",
            );
        }
    }

    let module = Module::new(&engine, wasm)?;

    if let Err(e) = task_manager.block_on(module_cache.save(hash, &engine, &module)) {
        tracing::warn!(
            %hash,
            error=&e as &dyn std::error::Error,
            "Unable to cache the compiled module",
        );
    }

    Ok(module)
}
