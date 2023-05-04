use std::{cell::RefCell, collections::HashMap};

use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, CompiledModuleCache};

std::thread_local! {
    static CACHED_MODULES: RefCell<HashMap<String, Module>>
        = RefCell::new(HashMap::new());
}

/// A cache that will cache modules in a thread-local variable.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ThreadLocalCache {}

impl ThreadLocalCache {
    fn lookup(&self, key: &str) -> Option<Module> {
        CACHED_MODULES.with(|m| m.borrow().get(key).cloned())
    }

    fn insert(&self, key: &str, module: &Module) {
        CACHED_MODULES.with(|m| m.borrow_mut().insert(key.to_string(), module.clone()));
    }
}

#[async_trait::async_trait]
impl CompiledModuleCache for ThreadLocalCache {
    async fn load(&self, key: &str, _engine: &Engine) -> Result<Module, CacheError> {
        self.lookup(key).ok_or(CacheError::NotFound)
    }

    async fn save(&self, key: &str, module: &Module) -> Result<(), CacheError> {
        self.insert(key, module);
        Ok(())
    }
}
