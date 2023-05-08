use std::{cell::RefCell, collections::HashMap};

use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

std::thread_local! {
    static CACHED_MODULES: RefCell<HashMap<ModuleHash, Module>>
        = RefCell::new(HashMap::new());
}

/// A cache that will cache modules in a thread-local variable.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ThreadLocalCache {}

impl ThreadLocalCache {
    fn lookup(&self, key: ModuleHash) -> Option<Module> {
        CACHED_MODULES.with(|m| m.borrow().get(&key).cloned())
    }

    fn insert(&self, key: ModuleHash, module: &Module) {
        CACHED_MODULES.with(|m| m.borrow_mut().insert(key, module.clone()));
    }
}

#[async_trait::async_trait]
impl ModuleCache for ThreadLocalCache {
    async fn load(&self, key: ModuleHash, _engine: &Engine) -> Result<Module, CacheError> {
        self.lookup(key).ok_or(CacheError::NotFound)
    }

    async fn save(&self, key: ModuleHash, module: &Module) -> Result<(), CacheError> {
        self.insert(key, module);
        Ok(())
    }
}
