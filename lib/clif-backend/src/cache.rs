use crate::relocation::{ExternalRelocation, TrapSink};

use hashbrown::HashMap;
use std::sync::Arc;
use wasmer_runtime_core::{
    backend::{sys::Memory, CacheGen},
    cache::{Artifact, Error},
    module::{ModuleInfo, ModuleInner},
    structures::Map,
    types::{LocalFuncIndex, SigIndex},
};

use serde_bench::{deserialize, serialize};

pub struct CacheGenerator {
    backend_cache: BackendCache,
    memory: Arc<Memory>,
}

impl CacheGenerator {
    pub fn new(backend_cache: BackendCache, memory: Arc<Memory>) -> Self {
        Self {
            backend_cache,
            memory,
        }
    }
}

impl CacheGen for CacheGenerator {
    fn generate_cache(
        &self,
        module: &ModuleInner,
    ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), Error> {
        let info = Box::new(module.info.clone());

        // Clone the memory to a new location. This could take a long time,
        // depending on the throughput of your memcpy implementation.
        let compiled_code = (*self.memory).clone();

        Ok((
            info,
            self.backend_cache.into_backend_data()?.into_boxed_slice(),
            compiled_code,
        ))
    }
}

#[derive(Serialize, Deserialize)]
pub struct TrampolineCache {
    #[serde(with = "serde_bytes")]
    pub code: Vec<u8>,
    pub offsets: HashMap<SigIndex, usize>,
}

#[derive(Serialize, Deserialize)]
pub struct BackendCache {
    pub external_relocs: Map<LocalFuncIndex, Box<[ExternalRelocation]>>,
    pub offsets: Map<LocalFuncIndex, usize>,
    pub trap_sink: Arc<TrapSink>,
    pub trampolines: TrampolineCache,
}

impl BackendCache {
    pub fn from_cache(cache: Artifact) -> Result<(ModuleInfo, Memory, Self), Error> {
        let (info, backend_data, compiled_code) = cache.consume();

        let backend_cache =
            deserialize(&backend_data).map_err(|e| Error::DeserializeError(e.to_string()))?;

        Ok((info, compiled_code, backend_cache))
    }

    pub fn into_backend_data(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = Vec::new();

        serialize(&mut buffer, self).map_err(|e| Error::SerializeError(e.to_string()))?;

        Ok(buffer)
    }
}
