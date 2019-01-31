use crate::relocation::{Relocation, TrapSink};

use wasmer_runtime_core::{
    cache::{Cache, Error},
    module::ModuleInfo,
    structures::Map,
    types::LocalFuncIndex,
};

use serde_bench::{deserialize, serialize};

#[derive(Serialize, Deserialize)]
pub struct BackendCache {
    pub relocations: Map<LocalFuncIndex, Box<[Relocation]>>,
    #[serde(with = "serde_bytes")]
    pub code: Vec<u8>,
    pub offsets: Map<LocalFuncIndex, usize>,
    pub trap_sink: TrapSink,
}

impl BackendCache {
    pub fn from_cache(cache: Cache) -> Result<(ModuleInfo, Self), Error> {
        let (info, backend_data) = cache.consume();

        let backend_cache = deserialize(backend_data.as_slice())
            .map_err(|e| Error::DeserializeError(e.to_string()))?;

        Ok((info, backend_cache))
    }

    pub fn into_backend_data(self) -> Result<Vec<u8>, Error> {
        let mut buffer = Vec::new();

        serialize(&mut buffer, &self).map_err(|e| Error::SerializeError(e.to_string()))?;

        Ok(buffer)
    }
}
