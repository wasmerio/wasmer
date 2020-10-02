use cfg_if::cfg_if;
use std::sync::Arc;
use wasmer::Engine;

/// this can be a wasmer-specific type with wasmer-specific functions for manipulating it
///
#[allow(non_camel_case_types)]
pub struct wasm_config_t {}

#[no_mangle]
pub extern "C" fn wasm_config_new() -> *mut wasm_config_t {
    todo!("wasm_config_new")
    //ptr::null_mut()
}

#[allow(non_camel_case_types)]
pub struct wasm_engine_t {
    pub(crate) inner: Arc<dyn Engine + Send + Sync>,
}

cfg_if! {
    if #[cfg(all(feature = "jit", feature = "compiler"))] {
        // Compiler JIT
        use wasmer_compiler::CompilerConfig;
        use wasmer_engine_jit::JIT;

        fn get_default_compiler_config() -> Box<dyn CompilerConfig> {
            cfg_if! {
                if #[cfg(feature = "cranelift")] {
                    Box::new(wasmer_compiler_cranelift::Cranelift::default())
                } else if #[cfg(feature = "llvm")] {
                    Box::new(wasmer_compiler_llvm::LLVM::default())
                } else if #[cfg(feature = "singlepass")] {
                    Box::new(wasmer_compiler_singlepass::Singlepass::default())
                } else {
                    compile_error!("Please enable one of the compiler backends")
                }
            }
        }

                #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::new(&*compiler_config).engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(feature = "jit")] {
        // Headless JIT
                #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(JIT::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else {
                #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            unimplemented!("The JITEngine is not attached");
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_engine_delete(_wasm_engine_address: Option<Box<wasm_engine_t>>) {}

#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    _config_ptr: *mut wasm_config_t,
) -> Box<wasm_engine_t> {
    wasm_engine_new()
}
