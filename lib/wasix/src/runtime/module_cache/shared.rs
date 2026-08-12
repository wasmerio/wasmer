use dashmap::DashMap;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache};
use wasmer_types::ModuleHash;

/// A [`ModuleCache`] based on a <code>[DashMap]</code> keyed by module hash, engine ID, and format.
#[derive(Debug, Default, Clone)]
pub struct SharedCache {
    modules: DashMap<SharedCacheKey, Module>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SharedCacheKey {
    module_hash: ModuleHash,
    deterministic_id: String,
    artifact_format: String,
}

impl SharedCache {
    pub fn new() -> SharedCache {
        SharedCache::default()
    }

    fn cache_key(key: ModuleHash, engine: &Engine) -> SharedCacheKey {
        SharedCacheKey {
            module_hash: key,
            deterministic_id: engine.deterministic_id(),
            artifact_format: engine.artifact_format(),
        }
    }
}

#[async_trait::async_trait]
impl ModuleCache for SharedCache {
    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let key = Self::cache_key(key, engine);

        match self.modules.get(&key) {
            Some(m) => {
                tracing::debug!("Cache hit!");
                Ok(m.value().clone())
            }

            None => Err(CacheError::NotFound),
        }
    }

    async fn contains(&self, key: ModuleHash, engine: &Engine) -> Result<bool, CacheError> {
        let key = Self::cache_key(key, engine);
        Ok(self.modules.contains_key(&key))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let key = Self::cache_key(key, engine);
        self.modules.insert(key, module.clone());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADD_WAT: &[u8] = br#"(
        module
            (func
                (export "add")
                (param $x i64)
                (param $y i64)
                (result i64)
                (i64.add (local.get $x) (local.get $y)))
        )"#;

    #[tokio::test]
    async fn round_trip_via_cache() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache = SharedCache::default();
        let key = ModuleHash::from_bytes([0; _]);

        cache.save(key, &engine, &module).await.unwrap();
        let round_tripped = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = round_tripped
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
