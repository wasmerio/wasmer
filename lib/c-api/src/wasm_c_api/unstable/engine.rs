#[cfg(feature = "compiler")]
use super::super::engine::get_default_compiler_config;
use super::super::engine::wasm_engine_t;
use super::target_lexicon::wasm_target_t;
use cfg_if::cfg_if;
use std::sync::Arc;
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;
use wasmer_engine::Engine;
#[cfg(feature = "jit")]
use wasmer_engine_jit::JIT;
#[cfg(feature = "native")]
use wasmer_engine_native::Native;
#[cfg(feature = "object-file")]
use wasmer_engine_object_file::ObjectFile;

cfg_if! {
    if #[cfg(all(feature = "jit", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>> {
            let target = target?;
            let compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::new(compiler_config).target(target.inner).engine());

            Some(Box::new(wasm_engine_t { inner: engine }))
        }
    } else if #[cfg(feature = "jit")] {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>> {
            let target = target?;
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::headless().target(target.inner).engine());

            Some(Box::new(wasm_engine_t { inner: engine }))
        }
    } else if #[cfg(all(feature = "native", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>> {
            let target = target?;
            let mut compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::new(compiler_config).target(target.inner).engine());

            Some(Box::new(wasm_engine_t { inner: engine }))
        }
    } else if #[cfg(feature = "native")] {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>>> {
            let target = target?;
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::headless().target(target.inner).engine());

            Some(Box::new(wasm_engine_t { inner: engine }))
        }
    }
    // There are currently no uses of the object-file engine + compiler from the C API.
    // So if we get here, we default to headless mode regardless of if `compiler` is enabled.
    else if #[cfg(feature = "object-file")] {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>> {
            let target = target?;
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(ObjectFile::headless().target(target.inner).engine());

            Some(Box::new(wasm_engine_t { inner: engine }))
        }
    } else {
        #[no_mangle]
        pub extern "C" fn wasm_new_engine_with_target(_target: Option<Box<wasm_target_t>>) -> Option<Box<wasm_engine_t>> {
            unimplemented!("No engine attached; You might want to recompile `wasmer_c_api` with for example `--feature jit`");
        }
    }
}
