use crate::{module::Module, new};
use blake3;
use std::str::FromStr;

#[derive(Debug)]
pub enum Error {
    DeserializeError(new::wasmer_engine::DeserializeError),
    SerializeError(new::wasmer_engine::SerializeError),
}

pub struct Artifact {
    new_module: new::wasmer::Module,
}

impl Artifact {
    pub(crate) fn new(new_module: new::wasmer::Module) -> Self {
        Self { new_module }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        self.new_module.serialize().map_err(Error::SerializeError)
    }

    pub unsafe fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        let store = Default::default();

        Ok(Self::new(
            new::wasmer::Module::deserialize(&store, bytes).map_err(Error::DeserializeError)?,
        ))
    }

    pub fn module(self) -> Module {
        Module::new(self.new_module)
    }

    pub fn info(&self) -> &new::wasmer_runtime::ModuleInfo {
        self.new_module.info()
    }
}

pub const WASMER_VERSION_HASH: &'static str =
    include_str!(concat!(env!("OUT_DIR"), "/wasmer_version_hash.txt"));

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WasmHash {
    new_hash: new::wasmer_cache::WasmHash,
}

impl WasmHash {
    pub fn generate(wasm_bytes: &[u8]) -> Self {
        let hash = blake3::hash(wasm_bytes);

        Self {
            new_hash: new::wasmer_cache::WasmHash::new(hash.into()),
        }
    }

    pub fn encode(self) -> String {
        self.new_hash.to_string()
    }

    pub fn decode(hex_str: &str) -> Result<Self, new::wasmer_engine::DeserializeError> {
        Ok(Self {
            new_hash: new::wasmer_cache::WasmHash::from_str(hex_str)?,
        })
    }
}
