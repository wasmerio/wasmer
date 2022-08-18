use std::sync::RwLock;
use std::{
    collections::HashMap,
    cell::RefCell,
    ops::DerefMut,
    path::PathBuf,
};
use derivative::*;

use bytes::Bytes;
use wasmer::{Module, AsStoreRef};
#[cfg(feature = "sys")]
use wasmer::Engine;
use wasmer_wasi_types::__WASI_CLOCK_MONOTONIC;

use crate::{WasiRuntimeImplementation, VirtualTaskManager};
use crate::syscalls::platform_clock_time_get;

use super::BinaryPackage;

pub const DEFAULT_COMPILED_PATH: &'static str = "~/.wasmer/compiled";
pub const DEFAULT_WEBC_PATH: &'static str = "~/.wasmer/webc";
pub const DEFAULT_CACHE_TIME: u128 = std::time::Duration::from_secs(30).as_nanos();

#[derive(Derivative)]
#[derivative(Debug)]
pub struct CachedCompiledModules {
    #[cfg(feature = "sys")]
    pub(crate) cached_modules: RwLock<HashMap<String, Module>>,
    pub(crate) cache_compile_dir: String,
    pub(crate) cache_webc: RwLock<HashMap<String, BinaryPackage>>,
    pub(crate) cache_webc_dir: String,
    #[cfg(feature = "sys")]
    #[derivative(Debug = "ignore")]
    pub(crate) engine: Engine,
}

impl Default
for CachedCompiledModules {
    fn default() -> Self {
        CachedCompiledModules::new(None, None)
    }
}

thread_local! {
    static THREAD_LOCAL_CACHED_MODULES: std::cell::RefCell<HashMap<String, Module>> 
        = RefCell::new(HashMap::new());
}

impl CachedCompiledModules
{
    #[cfg(feature = "sys")]
    fn new_engine() -> wasmer::Engine
    {
        // Build the features list
        let mut features = wasmer::Features::new();
        features.threads(true);
        features.memory64(true);
        features.bulk_memory(true);
        #[cfg(feature = "singlepass")]
        features.multi_value(false);

        // Choose the right compiler
        #[cfg(feature = "compiler-cranelift")]
        {
            let compiler = wasmer_compiler_cranelift::Cranelift::default();
            return wasmer_compiler::EngineBuilder::new(compiler)
                    .set_features(Some(features))
                    .engine();
        }
        #[cfg(all(not(feature = "compiler-cranelift"), feature = "compiler-llvm"))]
        {
            let compiler = wasmer_compiler_llvm::LLVM::default();
            return wasmer_compiler::EngineBuilder::new(compiler)
                .set_features(Some(features))
                .engine();
        }
        #[cfg(all(not(feature = "compiler-cranelift"), not(feature = "compiler-singlepass"), feature = "compiler-llvm"))]
        {
            let compiler = wasmer_compiler_singlepass::Singlepass::default();
            return wasmer_compiler::EngineBuilder::new(compiler)
                .set_features(Some(features))
                .engine();
        }
        #[cfg(all(not(feature = "compiler-cranelift"), not(feature = "compiler-singlepass"), not(feature = "compiler-llvm")))]
        panic!("wasmer not built with a compiler")
    }

    pub fn new(cache_compile_dir: Option<String>, cache_webc_dir: Option<String>) -> CachedCompiledModules {
        let cache_compile_dir = shellexpand::tilde(cache_compile_dir
            .as_ref()
            .map(|a| a.as_str())
            .unwrap_or_else(|| DEFAULT_COMPILED_PATH))
            .to_string();
        let _ = std::fs::create_dir_all(PathBuf::from(cache_compile_dir.clone()));

        let cache_webc_dir = shellexpand::tilde(cache_webc_dir
            .as_ref()
            .map(|a| a.as_str())
            .unwrap_or_else(|| DEFAULT_WEBC_PATH))
            .to_string();
        let _ = std::fs::create_dir_all(PathBuf::from(cache_webc_dir.clone()));
        
        #[cfg(feature = "sys")]
        let engine = Self::new_engine();

        CachedCompiledModules {
            #[cfg(feature = "sys")]
            cached_modules: RwLock::new(HashMap::default()),
            cache_compile_dir,
            cache_webc: RwLock::new(HashMap::default()),
            cache_webc_dir,
            #[cfg(feature = "sys")]
            engine
        }
    }

    #[cfg(feature = "sys")]
    pub fn new_store(&self) -> wasmer::Store
    {
        let engine = self.engine.clone();
        wasmer::Store::new(engine)
    }

    #[cfg(not(feature = "sys"))]
    pub fn new_store(&self) -> wasmer::Store
    {
        wasmer::Store::default()
    }

    pub fn get_webc(&self, webc: &str, runtime: &dyn WasiRuntimeImplementation, tasks: &dyn VirtualTaskManager) -> Option<BinaryPackage> {
        let name = webc.to_string();
        let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        
        // Fast path
        {
            let cache = self.cache_webc.read().unwrap();
            if let Some(data) = cache.get(&name) {
                let delta = now - data.when_cached;
                if delta <= DEFAULT_CACHE_TIME {
                    return Some(data.clone());
                }
            }
        }

        // Slow path
        let mut cache = self.cache_webc.write().unwrap();

        // Check the cache
        if let Some(data) = cache.get(&name) {
            let delta = now - data.when_cached;
            if delta <= DEFAULT_CACHE_TIME {
                return Some(data.clone());
            }
        }
        
        // Now try for the WebC
        let wapm_name = name.split_once(":").map(|a| a.0).unwrap_or_else(|| name.as_str());
        let cache_webc_dir = self.cache_webc_dir.as_str();
        if let Some(data) = crate::wapm::fetch_webc(cache_webc_dir, wapm_name, runtime, tasks)
        {
            // If the package is the same then don't replace it
            // as we don't want to duplicate the memory usage
            if let Some(existing) = cache.get_mut(&name) {
                if existing.hash() == data.hash() && existing.version == data.version {
                    existing.when_cached = now;
                    return Some(data.clone());
                }
            }
            cache.insert(name, data.clone());
            return Some(data);
        }

        // Not found
        None
    }

    pub fn get_compiled_module(&self, store: &impl AsStoreRef, data_hash: &str, compiler: &str) -> Option<Module> {
        let key = format!("{}-{}", data_hash, compiler);
        
        // fastest path
        {
            let module = THREAD_LOCAL_CACHED_MODULES.with(|cache| {
                let cache = cache.borrow();
                cache.get(&key).map(|m| m.clone())
            });
            if let Some(module) = module {
                return Some(module);
            }
        }

        // fast path
        #[cfg(feature = "sys")]
        {
            let cache = self.cached_modules.read().unwrap();
            if let Some(module) = cache.get(&key) {
                THREAD_LOCAL_CACHED_MODULES.with(|cache| {
                    let mut cache = cache.borrow_mut();
                    cache.insert(key.clone(), module.clone());
                });
                return Some(module.clone());
            }
        }

        // slow path
        let path = std::path::Path::new(self.cache_compile_dir.as_str()).join(format!("{}.bin", key).as_str());
        if let Ok(data) = std::fs::read(path) {
            let mut decoder = weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8);
            if let Ok(data) = decoder.decode(&data[..]) {
                let module_bytes = Bytes::from(data);

                // Load the module
                let module = unsafe { Module::deserialize(store, &module_bytes[..])
                    .unwrap()
                };

                #[cfg(feature = "sys")]
                {
                    let mut cache = self.cached_modules.write().unwrap();
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
        #[cfg(feature = "sys")]
        {
            let mut cache = self.cached_modules.write().unwrap();
            cache.insert(key.clone(), module.clone());
        }
        
        // We should also attempt to store it in the cache directory
        let compiled_bytes = module.serialize().unwrap();

        let path = std::path::Path::new(self.cache_compile_dir.as_str()).join(format!("{}.bin", key).as_str());
        let _ = std::fs::create_dir_all(path.parent().unwrap().clone());
        let mut encoder = weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8);
        if let Ok(compiled_bytes) = encoder.encode(&compiled_bytes[..]) {
            let _ = std::fs::write(path, &compiled_bytes[..]);
        }
    }
}
