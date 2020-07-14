//! The cache module provides the common data structures used by compiler backends to allow
//! serializing compiled wasm code to a binary format. The binary format can be persisted,
//! and loaded to allow skipping compilation and fast startup.

use crate::{
    get_global_store,
    module::{Module, ModuleInfo},
    new,
};
use std::str::FromStr;

pub use new::wasmer_cache::FileSystemCache;

/// Kinds of caching errors
#[derive(Debug)]
pub enum Error {
    /// An error deserializing bytes into a cache data structure.
    DeserializeError(new::wasmer_engine::DeserializeError),

    /// An error serializing bytes from a cache data structure.
    SerializeError(new::wasmer_engine::SerializeError),
}

/// Artifact are produced by caching, are serialized/deserialized to binaries, and contain
/// module info, backend metadata, and compiled code.
pub struct Artifact {
    new_module: new::wasmer::Module,
}

impl Artifact {
    pub(crate) fn new(new_module: new::wasmer::Module) -> Self {
        Self { new_module }
    }

    /// Serializes the `Artifact` into a vector of bytes
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        self.new_module.serialize().map_err(Error::SerializeError)
    }

    /// Deserializes an `Artifact` from the given byte slice.
    pub unsafe fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self::new(
            new::wasmer::Module::deserialize(&get_global_store(), bytes)
                .map_err(Error::DeserializeError)?,
        ))
    }

    /// Get the associated module to this artifact.
    pub fn module(self) -> Module {
        Module::new(self.new_module)
    }

    /// A reference to the `Artifact`'s stored `ModuleInfo`
    pub fn info(&self) -> &ModuleInfo {
        self.new_module.info()
    }
}

/// The hash of a wasm module.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WasmHash {
    new_hash: new::wasmer_cache::WasmHash,
}

impl WasmHash {
    /// Hash a wasm module.
    ///
    /// # Note
    ///
    /// This does no verification that the supplied data
    /// is, in fact, a wasm module.
    pub fn generate(wasm_bytes: &[u8]) -> Self {
        Self {
            new_hash: new::wasmer_cache::WasmHash::generate(wasm_bytes),
        }
    }

    /// Create the hexadecimal representation of the
    /// stored hash.
    pub fn encode(self) -> String {
        self.new_hash.to_string()
    }

    /// Create hash from hexadecimal representation
    pub fn decode(hex_str: &str) -> Result<Self, new::wasmer_engine::DeserializeError> {
        Ok(Self {
            new_hash: new::wasmer_cache::WasmHash::from_str(hex_str)?,
        })
    }
}

/// A unique ID generated from the version of Wasmer for use with cache versioning
pub const WASMER_VERSION_HASH: &'static str =
    include_str!(concat!(env!("OUT_DIR"), "/wasmer_version_hash.txt"));
