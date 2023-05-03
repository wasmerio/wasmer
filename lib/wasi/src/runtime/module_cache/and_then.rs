use wasmer::{Engine, Module};

use crate::{
    runtime::module_cache::{CacheError, CompiledModuleCache},
    VirtualTaskManager,
};

/// A [`CompiledModuleCache`] combinator which will try operations on one cache
/// and fall back to a secondary cache if they fail.
///
/// Constructed via [`CompiledModuleCache::and_then()`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AndThen<Primary, Secondary> {
    primary: Primary,
    secondary: Secondary,
}

impl<Primary, Secondary> AndThen<Primary, Secondary> {
    pub(crate) fn new(primary: Primary, secondary: Secondary) -> Self {
        AndThen { primary, secondary }
    }

    pub fn primary(&self) -> &Primary {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut Primary {
        &mut self.primary
    }

    pub fn secondary(&self) -> &Secondary {
        &self.secondary
    }

    pub fn secondary_mut(&mut self) -> &mut Secondary {
        &mut self.secondary
    }

    pub fn into_inner(self) -> (Primary, Secondary) {
        let AndThen { primary, secondary } = self;
        (primary, secondary)
    }
}

#[async_trait::async_trait]
impl<Primary, Secondary> CompiledModuleCache for AndThen<Primary, Secondary>
where
    Primary: CompiledModuleCache + Send + Sync,
    Secondary: CompiledModuleCache + Send + Sync,
{
    async fn load(
        &self,
        key: &str,
        engine: &Engine,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<Module, CacheError> {
        let primary_error = match self.primary.load(key, engine, task_manager).await {
            Ok(m) => return Ok(m),
            Err(e) => e,
        };

        if let Ok(m) = self.secondary.load(key, engine, task_manager).await {
            // Now we've got a module, let's make sure it ends up in the primary
            // cache too.
            if let Err(e) = self.primary.save(key, &m, task_manager).await {
                tracing::warn!(
                    key,
                    error = &e as &dyn std::error::Error,
                    "Unable to save a module to the primary cache",
                );
            }

            return Ok(m);
        }

        Err(primary_error)
    }

    async fn save(
        &self,
        key: &str,
        module: &Module,
        task_manager: &dyn VirtualTaskManager,
    ) -> Result<(), CacheError> {
        futures::try_join!(
            self.primary.save(key, module, task_manager),
            self.secondary.save(key, module, task_manager)
        )?;
        Ok(())
    }
}
