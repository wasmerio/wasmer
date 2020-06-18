#[cfg(all(feature = "compiler", feature = "engine"))]
use crate::tunables::Tunables;
#[cfg(all(feature = "compiler", feature = "engine"))]
use wasmer_compiler::CompilerConfig;

use std::sync::Arc;
use wasmer_engine::Engine;

#[derive(Clone)]
pub struct Store {
    engine: Arc<dyn Engine + Send + Sync>,
}

impl Store {
    pub fn new<E>(engine: &E) -> Store
    where
        E: Engine + ?Sized,
    {
        Store {
            engine: engine.cloned(),
        }
    }

    pub fn engine(&self) -> &Arc<dyn Engine + Send + Sync> {
        &self.engine
    }

    pub fn same(a: &Store, b: &Store) -> bool {
        a.engine.id() == b.engine.id()
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Store::same(self, other)
    }
}

// We only implement default if we have assigned a default compiler and engine
#[cfg(all(feature = "default-compiler", feature = "default-engine"))]
impl Default for Store {
    fn default() -> Store {
        // We store them on a function that returns to make
        // sure this function doesn't emit a compile error even if
        // more than one compiler is enabled.
        #[allow(unreachable_code)]
        fn get_config() -> impl CompilerConfig + Send + Sync {
            cfg_if::cfg_if! {
                if #[cfg(any(
                    all(feature = "default-llvm", any(feature = "default-cranelift", feature = "default-singlepass")),
                    all(feature = "default-cranelift", feature = "default-singlepass")
                ))] {
                    compile_error!("Only one compiler can be the default")
                } else if #[cfg(feature = "default-cranelift")] {
                    wasmer_compiler_cranelift::CraneliftConfig::default()
                } else if #[cfg(feature = "default-llvm")] {
                    wasmer_compiler_llvm::LLVMConfig::default()
                } else if #[cfg(feature = "default-singlepass")] {
                    wasmer_compiler_singlepass::SinglepassConfig::default()
                } else {
                    compile_error!("No compiler chosen")
                }
            }
        }

        #[allow(unreachable_code)]
        fn get_engine(config: impl CompilerConfig + Send + Sync) -> impl Engine + Send + Sync {
            cfg_if::cfg_if! {
                if #[cfg(all(
                    feature = "default-jit", feature = "default-native"
                ))] {
                    compile_error!("Only one engine can be the default")
                } else if #[cfg(feature = "default-jit")] {
                    wasmer_engine_jit::JIT::new(&config)
                        .tunables(Tunables::for_target)
                        .engine()
                } else if #[cfg(feature = "default-llvm")] {
                    wasmer_engine_native::Native::new(&config)
                        .tunables(Tunables::for_target)
                        .engine()
                } else {
                    compile_error!("No default engine chosen")
                }
            }
        }

        let config = get_config();
        let engine = get_engine(config);
        Store {
            engine: Arc::new(engine),
        }
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
