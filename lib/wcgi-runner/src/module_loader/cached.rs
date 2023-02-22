use tokio::sync::Mutex;

use crate::{
    module_loader::{ModuleLoader, ModuleLoaderContext},
    Error,
};

use super::LoadedModule;

pub(crate) struct Cached<L> {
    loader: L,
    invalidated: Box<dyn Fn() -> bool + Send + Sync>,
    cached: Mutex<Option<LoadedModule>>,
}

impl<L> Cached<L> {
    pub(crate) fn new(loader: L, invalidated: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            loader,
            invalidated: Box::new(invalidated),
            cached: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl<L: ModuleLoader> ModuleLoader for Cached<L> {
    async fn load(&self, ctx: ModuleLoaderContext<'_>) -> Result<LoadedModule, Error> {
        let mut cached = self.cached.lock().await;

        if (self.invalidated)() {
            // Throw away the previous value to make sure we will always load
            // the module again, even if invalidated() returns false in the
            // future or calling the inner loader fails further down.
            let _ = cached.take();
        }

        if let Some(module) = &*cached {
            // Cache hit!
            return Ok(module.clone());
        }

        let module = self.loader.load(ctx).await?;
        *cached = Some(module.clone());

        Ok(module)
    }
}
