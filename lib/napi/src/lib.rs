mod ctx;
mod env;
mod guest;
mod module;
mod snapi;
#[cfg(feature = "wasix")]
mod wasix;

use anyhow::{Context, Result};
use std::path::Path;
use wasmer::{FunctionEnv, Imports, Instance, Memory, TypedFunction};

pub use ctx::{NapiCtx, NapiCtxBuilder, NapiLimits, NapiRuntimeHooks, NapiSession};
pub use module::{load_wasix_module, LoadedWasm};
pub fn module_needs_napi(module: &wasmer::Module) -> bool {
    NapiCtx::module_needs_napi(module)
}
#[cfg(feature = "wasix")]
pub use wasix::{
    configure_runner_mounts, run_wasix_main_capture_stdio, run_wasix_main_capture_stdio_with_ctx,
    run_wasix_main_capture_stdout, run_wasix_main_capture_stdout_with_ctx, GuestMount,
};

pub(crate) use env::RuntimeEnv;
use guest::napi::{register_env_imports, register_napi_imports};
use module::{load_or_compile_module, make_store};

pub fn run_wasm_main_i32(wasm_path: &Path) -> Result<i32> {
    let wasm_bytes = std::fs::read(wasm_path)
        .with_context(|| format!("failed to read wasm file at {}", wasm_path.display()))?;
    let mut store = make_store();
    let module = load_or_compile_module(&store, &wasm_bytes)?;

    let memory_type = module
        .imports()
        .find_map(|import| {
            if import.module() == "env" && import.name() == "memory" {
                if let wasmer::ExternType::Memory(ty) = import.ty() {
                    return Some(*ty);
                }
            }
            None
        })
        .context("module does not import env.memory")?;
    let memory = Memory::new(&mut store, memory_type).context("failed to create memory")?;

    let func_env = FunctionEnv::new(&mut store, RuntimeEnv::with_memory(memory.clone()));

    let mut import_object = Imports::new();
    import_object.define("env", "memory", memory);
    register_env_imports(&mut store, &mut import_object);
    register_napi_imports(&mut store, &func_env, &mut import_object);

    let instance = Instance::new(&mut store, &module, &import_object)
        .context("failed to instantiate wasm module")?;
    let main_fn: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "main")
        .context("no main export found")?;
    let result = main_fn.call(&mut store).unwrap_or(-1);
    Ok(result)
}
