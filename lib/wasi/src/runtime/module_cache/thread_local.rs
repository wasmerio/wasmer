use std::{cell::RefCell, collections::HashMap};

use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

std::thread_local! {
    static CACHED_MODULES: RefCell<HashMap<(ModuleHash, String), Module>>
        = RefCell::new(HashMap::new());
}

/// A cache that will cache modules in a thread-local variable.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ThreadLocalCache {}

impl ThreadLocalCache {
    fn lookup(&self, key: ModuleHash, deterministic_id: &str) -> Option<Module> {
        let key = (key, deterministic_id.to_string());
        CACHED_MODULES.with(|m| m.borrow().get(&key).cloned())
    }

    fn insert(&self, key: ModuleHash, module: &Module, deterministic_id: &str) {
        let key = (key, deterministic_id.to_string());
        CACHED_MODULES.with(|m| m.borrow_mut().insert(key, module.clone()));
    }
}

#[async_trait::async_trait]
impl ModuleCache for ThreadLocalCache {
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        self.lookup(key, engine.deterministic_id())
            .ok_or(CacheError::NotFound)
    }

    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        self.insert(key, module, engine.deterministic_id());
        Ok(())
    }
}
