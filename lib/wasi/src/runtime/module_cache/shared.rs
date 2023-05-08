use dashmap::DashMap;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

/// A [`ModuleCache`] based on a <code>[DashMap]<[ModuleHash], [Module]></code>.
#[derive(Debug, Default, Clone)]
pub struct SharedCache {
    modules: DashMap<(ModuleHash, String), Module>,
}

impl SharedCache {
    pub fn new() -> SharedCache {
        SharedCache::default()
    }
}

#[async_trait::async_trait]
impl ModuleCache for SharedCache {
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let key = (key, engine.deterministic_id().to_string());
        self.modules
            .get(&key)
            .map(|m| m.value().clone())
            .ok_or(CacheError::NotFound)
    }

    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let key = (key, engine.deterministic_id().to_string());
        self.modules.insert(key, module.clone());

        Ok(())
    }
}
