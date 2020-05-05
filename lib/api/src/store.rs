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
    fn new_config(config: impl CompilerConfig) -> Self {
        let tunables = Tunables::for_target(config.target().triple());
        Self::new(&Engine::new(&config, tunables))
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
        let config = crate::DefaultCompilerConfig::default();
        Store::new_config(config)
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
