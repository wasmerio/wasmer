use crate::relocation::{ExternalRelocation, TrapSink};

use hashbrown::HashMap;
use wasmer_runtime_core::{
    backend::sys::Memory,
    cache::{Cache, Error},
    module::ModuleInfo,
    structures::Map,
    types::{LocalFuncIndex, SigIndex},
};

use serde_bench::{deserialize, serialize};

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
    pub trap_sink: TrapSink,
    pub trampolines: TrampolineCache,
}

impl BackendCache {
    pub fn from_cache(cache: Cache) -> Result<(ModuleInfo, Memory, Self), Error> {
        let (info, backend_data, compiled_code) = cache.consume();

        let backend_cache = deserialize(backend_data.as_slice())
            .map_err(|e| Error::DeserializeError(e.to_string()))?;

        Ok((info, compiled_code, backend_cache))
    }

    pub fn into_backend_data(self) -> Result<Vec<u8>, Error> {
        let mut buffer = Vec::new();

        serialize(&mut buffer, &self).map_err(|e| Error::SerializeError(e.to_string()))?;

        Ok(buffer)
    }
}
