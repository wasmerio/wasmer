use dashmap::DashMap;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, Key, ModuleCache};

/// A [`ModuleCache`] based on a <code>[DashMap]<[Key], [Module]></code>.
#[derive(Debug, Default, Clone)]
pub struct SharedCache {
    modules: DashMap<Key, Module>,
}

impl SharedCache {
    pub fn new() -> SharedCache {
        SharedCache::default()
    }
}

#[async_trait::async_trait]
impl ModuleCache for SharedCache {
    async fn load(&self, key: Key, _engine: &Engine) -> Result<Module, CacheError> {
        self.modules
            .get(&key)
            .map(|m| m.value().clone())
            .ok_or(CacheError::NotFound)
    }

    async fn save(&self, key: Key, module: &Module) -> Result<(), CacheError> {
        self.modules.insert(key, module.clone());

        Ok(())
    }
}
