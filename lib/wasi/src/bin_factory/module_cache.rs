use std::{cell::RefCell, collections::HashMap, ops::DerefMut, path::PathBuf, sync::RwLock};

use anyhow::Context;
use wasmer::Module;

use super::BinaryPackage;
use crate::WasiRuntime;

pub const DEFAULT_COMPILED_PATH: &str = "~/.wasmer/compiled";

#[derive(Debug)]
pub struct ModuleCache {
    pub(crate) cache_compile_dir: PathBuf,
    pub(crate) cached_modules: Option<RwLock<HashMap<String, Module>>>,
}

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

thread_local! {
    static THREAD_LOCAL_CACHED_MODULES: std::cell::RefCell<HashMap<String, Module>>
        = RefCell::new(HashMap::new());
}

impl ModuleCache {
    /// Create a new [`ModuleCache`].
    ///
    /// use_shared_cache enables a shared cache of modules in addition to a thread-local cache.
    pub fn new(cache_compile_dir: Option<PathBuf>, use_shared_cache: bool) -> ModuleCache {
        let cache_compile_dir = cache_compile_dir.unwrap_or_else(|| {
            PathBuf::from(shellexpand::tilde(DEFAULT_COMPILED_PATH).into_owned())
        });
        let _ = std::fs::create_dir_all(&cache_compile_dir);

        let cached_modules = if use_shared_cache {
            Some(RwLock::new(HashMap::default()))
        } else {
            None
        };

        ModuleCache {
            cached_modules,
            cache_compile_dir,
        }
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

    pub fn get_compiled_module(
        &self,
        engine: &impl wasmer::AsEngineRef,
        data_hash: &str,
        compiler: &str,
    ) -> Option<Module> {
        let key = format!("{}-{}", data_hash, compiler);

        // fastest path
        {
            let module = THREAD_LOCAL_CACHED_MODULES.with(|cache| {
                let cache = cache.borrow();
                cache.get(&key).cloned()
            });
            if let Some(module) = module {
                return Some(module);
            }
        }

        // fast path
        if let Some(cache) = &self.cached_modules {
            let cache = cache.read().unwrap();
            if let Some(module) = cache.get(&key) {
                THREAD_LOCAL_CACHED_MODULES.with(|cache| {
                    let mut cache = cache.borrow_mut();
                    cache.insert(key.clone(), module.clone());
                });
                return Some(module.clone());
            }
        }

        // slow path
        let path = self.cache_compile_dir.join(format!("{}.bin", key).as_str());
        if let Ok(data) = std::fs::read(path.as_path()) {
            tracing::trace!("bin file found: {:?} [len={}]", path.as_path(), data.len());
            let mut decoder = weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8);
            if let Ok(data) = decoder.decode(&data[..]) {
                let module_bytes = bytes::Bytes::from(data);

                // Load the module
                let module = match Module::deserialize_checked(engine, &module_bytes[..]) {
                    Ok(m) => m,
                    Err(err) => {
                        tracing::error!(
                            "failed to deserialize module with hash '{data_hash}': {err}"
                        );
                        return None;
                    }
                };

                if let Some(cache) = &self.cached_modules {
                    let mut cache = cache.write().unwrap();
                    cache.insert(key.clone(), module.clone());
                }

                THREAD_LOCAL_CACHED_MODULES.with(|cache| {
                    let mut cache = cache.borrow_mut();
                    cache.insert(key.clone(), module.clone());
                });
                return Some(module);
            }
        }

        // Not found
        tracing::trace!("bin file not found: {:?}", path);
        None
    }

    pub fn set_compiled_module(&self, data_hash: &str, compiler: &str, module: &Module) {
        let key = format!("{}-{}", data_hash, compiler);

        // Add the module to the local thread cache
        THREAD_LOCAL_CACHED_MODULES.with(|cache| {
            let mut cache = cache.borrow_mut();
            let cache = cache.deref_mut();
            cache.insert(key.clone(), module.clone());
        });

        // Serialize the compiled module into bytes and insert it into the cache
        if let Some(cache) = &self.cached_modules {
            let mut cache = cache.write().unwrap();
            cache.insert(key.clone(), module.clone());
        }

        // We should also attempt to store it in the cache directory
        let compiled_bytes = module.serialize().unwrap();

        let path = self.cache_compile_dir.join(format!("{}.bin", key).as_str());
        // TODO: forward error!
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let mut encoder = weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8);
        if let Ok(compiled_bytes) = encoder.encode(&compiled_bytes[..]) {
            let _ = std::fs::write(path, &compiled_bytes[..]);
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
