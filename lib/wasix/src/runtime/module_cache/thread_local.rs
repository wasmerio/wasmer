use std::{cell::RefCell, collections::HashMap};

use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache};
use wasmer_types::ModuleHash;

std::thread_local! {
    static CACHED_MODULES: RefCell<HashMap<(ModuleHash, String), Module>>
        = RefCell::new(HashMap::new());
}

/// A cache that will cache modules in a thread-local variable.
#[derive(Debug, Clone, Default)]
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
    #[tracing::instrument(level = "debug", skip_all, fields(%key))]
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        match self.lookup(key, &engine.deterministic_id()) {
            Some(m) => {
                tracing::debug!("Cache hit!");
                Ok(m)
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
        self.insert(key, module, &engine.deterministic_id());
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

    #[tokio::test(flavor = "current_thread")]
    async fn round_trip_via_cache() {
        let engine = Engine::default();
        let module = Module::new(&engine, ADD_WAT).unwrap();
        let cache = ThreadLocalCache::default();
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
