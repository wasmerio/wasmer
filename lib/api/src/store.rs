use std::sync::Arc;
use wasmer_compiler_cranelift::CraneliftConfig;
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
}

impl Default for Store {
    fn default() -> Store {
        Store::new(&Engine::new(&CraneliftConfig::default()))
    }
}

pub trait StoreObject {
    fn comes_from_same_store(&self, store: &Store) -> bool;
}
