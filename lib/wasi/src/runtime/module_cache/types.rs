use std::{fmt::Debug, ops::Deref};

use wasmer::{Engine, Module};

use crate::runtime::{module_cache::AndThen, VirtualTaskManager};

/// A cache for compiled WebAssembly modules.
///
/// ## Assumptions
///
/// Implementations can assume that cache keys are unique and that using the
/// same key to load or save will always result in the "same" module.
///
/// Implementations can also assume that [`CompiledModuleCache::load()`] will
/// be called more often than [`CompiledModuleCache::save()`] and optimise
/// their caching strategy accordingly.
#[async_trait::async_trait]
pub trait CompiledModuleCache: Debug + Send + Sync {
    async fn load(
        &self,
        key: &str,
        engine: &Engine,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<Module, CacheError>;

    async fn save(
        &self,
        key: &str,
        module: &Module,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<(), CacheError>;

    /// Chain a second cache onto this one.
    ///
    /// The general assumption is that each subsequent cache in the chain will
    /// be significantly slower than the previous one.
    ///
    /// ```rust
    /// use wasmer_wasix::runtime::module_cache::{
    ///     CompiledModuleCache, ThreadLocalCache, OnDiskCache, SharedCache,
    /// };
    ///
    /// let cache = ThreadLocalCache::default()
    ///     .and_then(SharedCache::default())
    ///     .and_then(OnDiskCache::new("~/.local/cache"));
    /// ```
    fn and_then<C>(self, other: C) -> AndThen<Self, C>
    where
        Self: Sized,
        C: CompiledModuleCache,
    {
        AndThen::new(self, other)
    }
}

#[async_trait::async_trait]
impl<D, C> CompiledModuleCache for D
where
    D: Deref<Target = C> + Debug + Send + Sync,
    C: CompiledModuleCache + ?Sized,
{
    async fn load(
        &self,
        key: &str,
        engine: &Engine,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<Module, CacheError> {
        (**self).load(key, engine, task_manager).await
    }
    async fn save(
        &self,
        key: &str,
        module: &Module,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<(), CacheError> {
        (**self).save(key, module, task_manager).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The item was not found.
    #[error("Not found")]
    NotFound,
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_object_safe() {
        let _: Option<Box<dyn CompiledModuleCache>> = None;
    }
}
