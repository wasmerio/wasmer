use std::{
    sync::{
        Arc, RwLock,
    },
    ops::{
        Deref
    }, collections::HashMap,
};
use derivative::Derivative;

mod binary_package;
mod cached_modules;
mod exec;

pub use binary_package::*;
pub use cached_modules::*;
pub use exec::spawn_exec;
pub use exec::spawn_exec_module;
pub(crate) use exec::SpawnedProcess;

use sha2::*;

use crate::{
    WasiState,
    WasiRuntimeImplementation,
    builtins::BuiltIns
};

#[derive(Derivative, Clone)]
pub struct BinFactory {
    pub(crate) state: Arc<WasiState>,
    pub(crate) builtins: BuiltIns,
    runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    pub(crate) cache: Arc<CachedCompiledModules>,
    pub(crate) local: Arc<RwLock<HashMap<String, Option<BinaryPackage>>>>,
}

impl BinFactory {
    pub fn new(
        state: Arc<WasiState>,
        compiled_modules: Arc<CachedCompiledModules>,
        runtime: Arc<dyn WasiRuntimeImplementation + Send + Sync + 'static>,
    ) -> BinFactory {
        BinFactory {
            state,
            builtins: BuiltIns::new(runtime.clone(), compiled_modules.clone()),
            runtime,
            cache: compiled_modules,
            local: Arc::new(RwLock::new(HashMap::new()))
        }
    }

    pub fn runtime(&self) -> &dyn WasiRuntimeImplementation {
        self.runtime.deref()
    }

    pub fn set_binary(&self, name: &str, binary: BinaryPackage) {
        let mut cache = self.local.write().unwrap();
        cache.insert(name.to_string(), Some(binary));
    }

    pub fn get_binary(&self, name: &str) -> Option<BinaryPackage> {
        let name = name.to_string();

        // Fast path
        {
            let cache = self.local.read().unwrap();
            if let Some(data) = cache.get(&name) {
                return data.clone();
            }
        }

        // Slow path
        let mut cache = self.local.write().unwrap();

        // Check the cache
        if let Some(data) = cache.get(&name) {
            return data.clone();
        }
        
        // Check the filesystem for the file
        if name.starts_with("/") {
            if let Ok(mut file) = self.state
                .fs_new_open_options()
                .read(true)
                .open(name.clone())
            {
                // Read the file
                let mut data = Vec::with_capacity(file.size() as usize);
                if let Ok(_) = file.read_to_end(&mut data)
                {
                    let package_name = name.split("/").last().unwrap_or_else(|| name.as_str());
                    let data = BinaryPackage::new(package_name, data.into());
                    cache.insert(name, Some(data.clone()));
                    return Some(data);
                }
            }
        }

        // NAK
        cache.insert(name, None);
        return None;
    }
}

pub fn hash_of_binary(data: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::default();
    hasher.update(data.as_ref());
    let hash = hasher.finalize();
    hex::encode(&hash[..])
}
