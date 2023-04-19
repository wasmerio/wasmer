use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::DerefMut,
    path::PathBuf,
    sync::RwLock,
};

use wasmer::Module;
use wasmer_wasix_types::wasi::Snapshot0Clockid;

use super::BinaryPackage;
use crate::{syscalls::platform_clock_time_get, WasiRuntime};

pub const DEFAULT_COMPILED_PATH: &str = "~/.wasmer/compiled";
pub const DEFAULT_WEBC_PATH: &str = "~/.wasmer/webc";
pub const DEFAULT_CACHE_TIME: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug)]
pub struct ModuleCache {
    pub(crate) cache_compile_dir: PathBuf,
    pub(crate) cached_modules: Option<RwLock<HashMap<String, Module>>>,

    pub(crate) cache_webc: RwLock<HashMap<String, BinaryPackage>>,
    pub(crate) cache_webc_dir: PathBuf,

    pub(crate) cache_time: std::time::Duration,
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
        ModuleCache::new(None, None, true)
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
    pub fn new(
        cache_compile_dir: Option<PathBuf>,
        cache_webc_dir: Option<PathBuf>,
        use_shared_cache: bool,
    ) -> ModuleCache {
        let cache_compile_dir = cache_compile_dir.unwrap_or_else(|| {
            PathBuf::from(shellexpand::tilde(DEFAULT_COMPILED_PATH).into_owned())
        });
        let _ = std::fs::create_dir_all(&cache_compile_dir);

        let cache_webc_dir = cache_webc_dir
            .unwrap_or_else(|| PathBuf::from(shellexpand::tilde(DEFAULT_WEBC_PATH).into_owned()));
        let _ = std::fs::create_dir_all(&cache_webc_dir);

        let cached_modules = if use_shared_cache {
            Some(RwLock::new(HashMap::default()))
        } else {
            None
        };

        ModuleCache {
            cached_modules,
            cache_compile_dir,
            cache_webc: RwLock::new(HashMap::default()),
            cache_webc_dir,
            cache_time: DEFAULT_CACHE_TIME,
        }
    }

    /// Adds a package manually to the module cache
    pub fn add_webc(&self, webc: &str, package: BinaryPackage) {
        let mut cache = self.cache_webc.write().unwrap();
        cache.insert(webc.to_string(), package);
    }

    // TODO: should return Result<_, anyhow::Error>
    pub fn get_webc(&self, webc: &str, runtime: &dyn WasiRuntime) -> Option<BinaryPackage> {
        let name = webc.to_string();

        // Fast path
        {
            let cache = self.cache_webc.read().unwrap();
            if let Some(data) = cache.get(&name) {
                if let Some(when_cached) = data.when_cached.as_ref() {
                    /* get the current platform time at this point because of the lock conflict
                     * now time remains the same and the lock from a differnt thread can cause when_cached to go ahead
                     * and cause a panic
                     */
                    let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000)
                        .unwrap() as u128;
                    let delta = now.saturating_sub(*when_cached); //saturating sub for delta and  to prevent panic
                    if delta <= self.cache_time.as_nanos() {
                        return Some(data.clone());
                    }
                } else {
                    return Some(data.clone());
                }
            }
        }

        // Slow path
        let mut cache = self.cache_webc.write().unwrap();
        self.get_webc_slow(webc, runtime, cache.deref_mut())
    }

    fn get_webc_slow(
        &self,
        webc: &str,
        runtime: &dyn WasiRuntime,
        cache: &mut HashMap<String, BinaryPackage>,
    ) -> Option<BinaryPackage> {
        let name = webc.to_string();
        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;

        // Check the cache (again)
        if let Some(data) = cache.get(&name) {
            if let Some(when_cached) = data.when_cached.as_ref() {
                let delta = now - *when_cached;
                if delta <= self.cache_time.as_nanos() {
                    return Some(data.clone());
                }
            } else {
                return Some(data.clone());
            }
        }

        // Now try for the WebC
        {
            let wapm_name = name
                .split_once(':')
                .map(|a| a.0)
                .unwrap_or_else(|| name.as_str());
            if let Ok(mut data) =
                crate::wapm::fetch_webc_task(&self.cache_webc_dir, wapm_name, runtime)
            {
                // If the binary has no entry but it inherits from another module
                // that does have an entry then we fall back to that inherited entry point
                // (this convention is recursive down the list of inheritance until it finds the first entry point)
                let mut already: HashSet<String> = Default::default();
                while data.entry.is_none() {
                    let mut inherits = data.uses.iter().filter_map(|webc| {
                        if !already.contains(webc) {
                            already.insert(webc.clone());
                            self.get_webc_slow(webc, runtime, cache)
                        } else {
                            None
                        }
                    });
                    if let Some(inherits) = inherits.next() {
                        data.entry = inherits.entry.clone();
                    } else {
                        break;
                    }
                }

                // If the package is the same then don't replace it
                // as we don't want to duplicate the memory usage
                if let Some(existing) = cache.get_mut(&name) {
                    if existing.hash() == data.hash() && existing.version == data.version {
                        existing.when_cached = Some(now);
                        return Some(existing.clone());
                    }
                }
                cache.insert(name, data.clone());
                return Some(data);
            }
        }

        // If we have an old one that use that (ignoring the TTL)
        if let Some(data) = cache.get(&name) {
            return Some(data.clone());
        }

        // Otherwise - its not found
        None
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

        let mut cache = ModuleCache::new(None, None, true);
        cache.cache_time = std::time::Duration::from_millis(500);

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
