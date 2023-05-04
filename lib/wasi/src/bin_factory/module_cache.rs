use std::path::PathBuf;

use anyhow::Context;
use wasmer::Module;

use super::BinaryPackage;
use crate::{runtime::module_cache::CompiledModuleCache, WasiRuntime};

pub const DEFAULT_COMPILED_PATH: &str = "~/.wasmer/compiled";

#[derive(Debug)]
pub struct ModuleCache(Box<dyn CompiledModuleCache>);

// FIXME: remove impls!
// Added as a stopgap to get the crate to compile again with the "js" feature.
// wasmer::Module holds a JsValue, which makes it non-sync.
#[cfg(feature = "js")]
unsafe impl Send for ModuleCache {}
#[cfg(feature = "js")]
unsafe impl Sync for ModuleCache {}

impl Default for ModuleCache {
    fn default() -> Self {
        ModuleCache::new(None, true)
    }
}

impl ModuleCache {
    /// Create a new [`ModuleCache`].
    ///
    /// use_shared_cache enables a shared cache of modules in addition to a thread-local cache.
    pub fn new(cache_compile_dir: Option<PathBuf>, _use_shared_cache: bool) -> ModuleCache {
        let cache_compile_dir = cache_compile_dir.unwrap_or_else(|| {
            PathBuf::from(shellexpand::tilde(DEFAULT_COMPILED_PATH).into_owned())
        });

        let cache = crate::runtime::module_cache::default_cache(&cache_compile_dir);

        ModuleCache(Box::new(cache))
    }

    pub fn get_webc(
        &self,
        webc: &str,
        runtime: &dyn WasiRuntime,
    ) -> Result<BinaryPackage, anyhow::Error> {
        let ident = webc.parse().context("Unable to parse the package name")?;
        let resolver = runtime.package_resolver();
        let client = runtime
            .http_client()
            .context("No HTTP client available")?
            .clone();

        runtime.task_manager().block_on(async move {
            resolver
                .resolve_package(&ident, &*client)
                .await
                .with_context(|| format!("An error occurred while fetching \"{webc}\""))
        })
    }

    pub async fn get_compiled_module(
        &self,
        runtime: &dyn WasiRuntime,
        data_hash: &str,
        compiler: &str,
    ) -> Option<Module> {
        let key = format!("{}-{}", data_hash, compiler);
        let engine = runtime.engine()?;

        self.0.load(&key, &engine).await.ok()
    }

    pub async fn set_compiled_module(&self, data_hash: &str, compiler: &str, module: &Module) {
        let key = format!("{}-{}", data_hash, compiler);

        let result = self.0.save(&key, module).await;

        if let Err(e) = result {
            tracing::warn!(
                data_hash,
                compiler,
                error = &e as &dyn std::error::Error,
                "Unable to cache the module",
            );
        }
    }
}

#[cfg(test)]
#[cfg(feature = "sys")]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tracing_subscriber::filter::LevelFilter;

    use crate::{runtime::task_manager::tokio::TokioTaskManager, PluggableRuntime};

    use super::*;

    #[test]
    fn test_module_cache() {
        let _ = tracing_subscriber::fmt()
            .pretty()
            .with_test_writer()
            .with_max_level(LevelFilter::INFO)
            .try_init();

        let cache = ModuleCache::new(None, true);

        let rt = PluggableRuntime::new(Arc::new(TokioTaskManager::shared()));
        let tasks = rt.task_manager();

        let mut store = Vec::new();
        for _ in 0..2 {
            let webc = cache.get_webc("sharrattj/dash", &rt).unwrap();
            store.push(webc);
            tasks
                .runtime()
                .block_on(tasks.sleep_now(Duration::from_secs(1)));
        }
    }
}
