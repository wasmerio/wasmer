use dashmap::DashMap;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache};

/// A [`ModuleCache`] based on a <code>[DashMap]<[String], [Module]></code>.
#[derive(Debug, Default, Clone)]
pub struct SharedCache {
    modules: DashMap<String, Module>,
}

impl SharedCache {
    pub fn new() -> SharedCache {
        SharedCache::default()
    }
}

#[async_trait::async_trait]
impl ModuleCache for SharedCache {
    async fn load(&self, key: &str, _engine: &Engine) -> Result<Module, CacheError> {
        self.modules
            .get(key)
            .map(|m| m.value().clone())
            .ok_or(CacheError::NotFound)
    }

    async fn save(&self, key: &str, module: &Module) -> Result<(), CacheError> {
        self.modules.insert(key.to_string(), module.clone());

        Ok(())
    }
}
