use crate::tunables::Tunables;
use std::sync::Arc;
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;
use wasmer_jit::JITEngine;

pub type Engine = JITEngine;

#[derive(Clone)]
pub struct Store {
    engine: Arc<Engine>,
}

impl Store {
    pub fn new(engine: &Engine) -> Store {
        Store {
            engine: Arc::new(engine.clone()),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn same(a: &Store, b: &Store) -> bool {
        Arc::ptr_eq(&a.engine, &b.engine)
    }

    #[cfg(feature = "compiler")]
    fn new_config(config: Box<dyn CompilerConfig>) -> Self {
        let tunables = Tunables::for_target(config.target().triple());
        Self::new(&Engine::new(&*config, tunables))
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Store::same(self, other)
    }
}

// We only implement default if we have assigned a default compiler
#[cfg(feature = "compiler")]
impl Default for Store {
    fn default() -> Store {
        // We store them on a function that returns to make
        // sure this function doesn't emit a compile error even if
        // more than one compiler is enabled.
        #[allow(unreachable_code)]
        fn get_config() -> Box<dyn CompilerConfig> {
            #[cfg(feature = "cranelift")]
            return Box::new(wasmer_compiler_cranelift::CraneliftConfig::default());

            #[cfg(feature = "llvm")]
            return Box::new(wasmer_compiler_llvm::LLVMConfig::default());

            #[cfg(feature = "singlepass")]
            return Box::new(wasmer_compiler_singlepass::SinglepassConfig::default());
        }
        Store::new_config(get_config())
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
