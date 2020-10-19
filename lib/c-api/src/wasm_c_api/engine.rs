use cfg_if::cfg_if;
use std::sync::Arc;
use wasmer::Engine;
#[cfg(feature = "jit")]
use wasmer_engine_jit::JIT;
#[cfg(feature = "native")]
use wasmer_engine_native::Native;
#[cfg(feature = "object-file")]
use wasmer_engine_object_file::ObjectFile;

/// this can be a wasmer-specific type with wasmer-specific functions for manipulating it
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub enum wasmer_compiler_t {
    CRANELIFT = 0,
    LLVM = 1,
    SINGLEPASS = 2,
}

impl Default for wasmer_compiler_t {
    fn default() -> Self {
        cfg_if! {
            if #[cfg(feature = "cranelift")] {
                Self::CRANELIFT
            } else if #[cfg(feature = "llvm")] {
                Self::LLVM
            } else if #[cfg(feature = "singlepass")] {
                Self::SINGLEPASS
            } else {
                compile_error!("Please enable one of the compiler backends")
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub enum wasmer_engine_t {
    JIT = 0,
    NATIVE = 1,
    OBJECT_FILE = 2,
}

impl Default for wasmer_engine_t {
    fn default() -> Self {
        cfg_if! {
            if #[cfg(feature = "jit")] {
                Self::JIT
            } else if #[cfg(feature = "native")] {
                Self::NATIVE
            } else if #[cfg(feature = "object-file")] {
                Self::OBJECT_FILE
            } else {
                compile_error!("Please enable one of the engines")
            }
        }
    }
}

/// cbindgen:ignore
/// this can be a wasmer-specific type with wasmer-specific functions for manipulating it
#[derive(Debug, Default)]
#[repr(C)]
pub struct wasm_config_t {
    compiler: wasmer_compiler_t,
    engine: wasmer_engine_t,
}

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_config_new() -> Box<wasm_config_t> {
    Box::new(wasm_config_t::default())
}

#[no_mangle]
pub extern "C" fn wasm_config_set_compiler(
    config: &mut wasm_config_t,
    compiler: wasmer_compiler_t,
) {
    config.compiler = compiler;
}

#[no_mangle]
pub extern "C" fn wasm_config_set_engine(config: &mut wasm_config_t, engine: wasmer_engine_t) {
    config.engine = engine;
}

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub struct wasm_engine_t {
    pub(crate) inner: Arc<dyn Engine + Send + Sync>,
}

// Compiler JIT
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;
#[cfg(feature = "compiler")]
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

cfg_if! {
    if #[cfg(all(feature = "jit", feature = "compiler"))] {
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
    else if #[cfg(all(feature = "native", feature = "compiler"))] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let mut compiler_config: Box<dyn CompilerConfig> = get_default_compiler_config();
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::new(&mut *compiler_config).engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else if #[cfg(feature = "native")] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(Native::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    // There are currently no uses of the object-file engine + compiler from the C API.
    // So if we get here, we default to headless mode regardless of if `compiler` is enabled.
    else if #[cfg(feature = "object-file")] {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            let engine: Arc<dyn Engine + Send + Sync> = Arc::new(ObjectFile::headless().engine());
            Box::new(wasm_engine_t { inner: engine })
        }
    }
    else {
        #[no_mangle]
        pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
            unimplemented!("The JITEngine is not attached; You might want to recompile `wasmer_c_api` with `--feature jit`");
        }
    }
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_engine_delete(_wasm_engine_address: Option<Box<wasm_engine_t>>) {}

/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    config: Box<wasm_config_t>,
) -> Option<Box<wasm_engine_t>> {
    // TODO: return useful error messages in failure branches
    cfg_if! {
        if #[cfg(feature = "compiler")] {
            #[allow(unused_mut)]
            let mut compiler_config: Box<dyn CompilerConfig> = match config.compiler {
                wasmer_compiler_t::CRANELIFT => {
                    cfg_if! {
                        if #[cfg(feature = "cranelift")] {
                            Box::new(wasmer_compiler_cranelift::Cranelift::default())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_compiler_t::LLVM => {
                    cfg_if! {
                        if #[cfg(feature = "llvm")] {
                            Box::new(wasmer_compiler_llvm::LLVM::default())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_compiler_t::SINGLEPASS => {
                    cfg_if! {
                        if #[cfg(feature = "singlepass")] {
                            Box::new(wasmer_compiler_singlepass::Singlepass::default())
                        } else {
                            return None;
                        }
                    }
                },
            };

            let inner: Arc<dyn Engine + Send + Sync> = match config.engine {
                wasmer_engine_t::JIT => {
                    cfg_if! {
                        if #[cfg(feature = "jit")] {
                            Arc::new(JIT::new(&*compiler_config).engine())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_engine_t::NATIVE => {
                    cfg_if! {
                        if #[cfg(feature = "native")] {
                            Arc::new(Native::new(&mut *compiler_config).engine())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_engine_t::OBJECT_FILE => {
                    cfg_if! {
                        // There are currently no uses of the object-file engine + compiler from the C API.
                        // So we run in headless mode.
                        if #[cfg(feature = "object-file")] {
                            Arc::new(ObjectFile::headless().engine())
                        } else {
                            return None;
                        }
                    }
                },
            };
            Some(Box::new(wasm_engine_t { inner }))
        } else {
            let inner: Arc<dyn Engine + Send + Sync> = match config.engine {
                wasmer_engine_t::JIT => {
                    cfg_if! {
                        if #[cfg(feature = "jit")] {
                            Arc::new(JIT::headless().engine())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_engine_t::NATIVE => {
                    cfg_if! {
                        if #[cfg(feature = "native")] {
                            Arc::new(Native::headless().engine())
                        } else {
                            return None;
                        }
                    }
                },
                wasmer_engine_t::OBJECT_FILE => {
                    cfg_if! {
                        if #[cfg(feature = "object-file")] {
                            Arc::new(ObjectFile::headless().engine())
                        } else {
                            return None;
                        }
                    }
                },
            };
            Some(Box::new(wasm_engine_t { inner }))
        }
    }
}
