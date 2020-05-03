use crate::tunables::Tunables;
use std::sync::Arc;
use wasmer_compiler::CompilerConfig;
use wasmer_engine::Engine;
use wasmer_jit::JITEngine;

#[derive(Clone)]
pub struct Store {
    engine: Arc<dyn Engine>,
}

impl Store {
    pub fn new(engine: Arc<dyn Engine>) -> Store {
        Store { engine }
    }

    pub fn engine(&self) -> &Arc<dyn Engine> {
        &self.engine
    }

    pub fn same(a: &Store, b: &Store) -> bool {
        Arc::ptr_eq(&a.engine, &b.engine)
    }

    #[cfg(any(
        feature = "default-compiler-singlepass",
        feature = "default-compiler-cranelift",
        feature = "default-compiler-llvm",
    ))]
    pub fn default_compiler_config() -> impl CompilerConfig {
        #[cfg(any(
            all(
                feature = "default-compiler-llvm",
                any(
                    feature = "default-compiler-cranelift",
                    feature = "default-compiler-singlepass"
                )
            ),
            all(
                feature = "default-compiler-cranelift",
                feature = "default-compiler-singlepass"
            )
        ))]
        compile_error!(
            "The `default-compiler-X` features are mutually exclusive. Please choose just one"
        );

        #[cfg(feature = "default-compiler-cranelift")]
        return wasmer_compiler_cranelift::CraneliftConfig::default();

        #[cfg(feature = "default-compiler-llvm")]
        return wasmer_compiler_llvm::LLVMConfig::default();

        #[cfg(feature = "default-compiler-singlepass")]
        return wasmer_compiler_singlepass::SinglepassConfig::default();
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Store::same(self, other)
    }
}

// We only implement default if we have assigned a default compiler
#[cfg(any(
    feature = "default-compiler-singlepass",
    feature = "default-compiler-cranelift",
    feature = "default-compiler-llvm",
))]
impl Default for Store {
    fn default() -> Store {
        let config = Self::default_compiler_config();
        let tunables = Tunables::for_target(config.target().triple());
        Store::new(Arc::new(JITEngine::new(&config, tunables)))
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
