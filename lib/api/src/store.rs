use crate::tunables::Tunables;
use std::sync::Arc;
use wasmer_compiler::CompilerConfig;
use wasmer_engine::Engine;

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

    #[cfg(all(
        any(
            feature = "default-compiler-singlepass",
            feature = "default-compiler-cranelift",
            feature = "default-compiler-llvm",
        ),
        any(feature = "default-engine-jit", feature = "default-engine-native",)
    ))]
    pub fn default_engine() -> impl Engine {
        #[cfg(all(feature = "default-engine-jit", feature = "default-engine-native",))]
        compile_error!(
            "The `default-engine-X` features are mutually exclusive. Please choose just one"
        );

        let config = Self::default_compiler_config();
        let tunables = Tunables::for_target(config.target().triple());

        #[cfg(feature = "engine-jit")]
        return wasmer_engine_jit::JITEngine::new(&config, tunables);
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Store::same(self, other)
    }
}

// We only implement default if we have assigned a default compiler
#[cfg(all(
    any(
        feature = "default-compiler-singlepass",
        feature = "default-compiler-cranelift",
        feature = "default-compiler-llvm",
    ),
    any(feature = "default-engine-jit", feature = "default-engine-native",)
))]
impl Default for Store {
    fn default() -> Store {
        let engine = Self::default_engine();
        Store::new(Arc::new(engine))
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
