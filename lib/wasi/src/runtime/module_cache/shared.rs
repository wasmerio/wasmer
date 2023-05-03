use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use wasmer::{Engine, Module};

use crate::{
    runtime::module_cache::{CacheError, CompiledModuleCache},
    VirtualTaskManager,
};

/// A [`CompiledModuleCache`] based on a
/// <code>[Arc]<[RwLock]<[HashMap]<[String], [Module]>>></code> that can be
/// shared.
#[derive(Debug, Default, Clone)]
pub struct SharedCache {
    modules: Arc<RwLock<HashMap<String, Module>>>,
}

impl SharedCache {
    pub fn new() -> SharedCache {
        SharedCache::default()
    }

    pub fn from_existing_modules(modules: Arc<RwLock<HashMap<String, Module>>>) -> Self {
        SharedCache { modules }
    }
}

#[async_trait::async_trait]
impl CompiledModuleCache for SharedCache {
    async fn load(
        &self,
        key: &str,
        _engine: &Engine,
        _task_manager: &dyn VirtualTaskManager,
    ) -> Result<Module, CacheError> {
        let modules = self.modules.read().await;

        modules.get(key).cloned().ok_or(CacheError::NotFound)
    }

    async fn save(
        &self,
        key: &str,
        module: &Module,
        _task_manager: &dyn VirtualTaskManager,
    ) -> Result<(), CacheError> {
        let module = module.clone();
        let key = key.to_string();

        self.modules.write().await.insert(key, module);

        Ok(())
    }
}
