use dashmap::DashMap;
use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache};
use wasmer_types::ModuleHash;

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
    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        let key = (key, engine.deterministic_id());

        match self.modules.get(&key) {
            Some(m) => {
                tracing::debug!("Cache hit!");
                Ok(m.value().clone())
            }

            None => Err(CacheError::NotFound),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
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
        let key = ModuleHash::xxhash_from_bytes([0; 8]);

        cache.save(key, &engine, &module).await.unwrap();
        let round_tripped = cache.load(key, &engine).await.unwrap();

        let exports: Vec<_> = round_tripped
            .exports()
            .map(|export| export.name().to_string())
            .collect();
        assert_eq!(exports, ["add"]);
    }
}
