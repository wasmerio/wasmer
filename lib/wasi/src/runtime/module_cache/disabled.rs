use wasmer::{Engine, Module};

use crate::runtime::module_cache::{CacheError, ModuleCache};

/// A cache that always fails.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct Disabled;

#[async_trait::async_trait]
impl ModuleCache for Disabled {
    async fn load(&self, _key: &str, _engine: &Engine) -> Result<Module, CacheError> {
        Err(CacheError::NotFound)
    }

    async fn save(&self, _key: &str, _module: &Module) -> Result<(), CacheError> {
        Err(CacheError::NotFound)
    }
}
