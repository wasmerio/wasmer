use shared_buffer::OwnedBuffer;
use wasmer_types::ModuleHash;

use crate::bin_factory::BinaryPackageCommand;

/// A wrapper around Webassembly code and its hash.
///
/// Allows passing around WASM code and it's hash without the danger of
/// using a wrong hash.
///
/// Safe by construction: can only be created from a [`BinaryCommand`], which
/// already has the hash embedded, or from bytes that will be hashed in the
/// constructor.
///
/// Can be cloned cheaply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashedModuleData {
    hash: ModuleHash,
    wasm: OwnedBuffer,
}

impl HashedModuleData {
    pub fn new_sha256(bytes: impl Into<OwnedBuffer>) -> Self {
        let wasm = bytes.into();
        let hash = ModuleHash::sha256(&wasm);
        Self { hash, wasm }
    }

    /// Create new [`HashedModuleData`] from the given bytes, hashing the
    /// the bytes into a [`ModuleHash`] with xxhash.
    pub fn new_xxhash(bytes: impl Into<OwnedBuffer>) -> Self {
        let wasm = bytes.into();
        let hash = ModuleHash::xxhash(&wasm);
        Self { hash, wasm }
    }

    /// Create new [`HashedModuleData`] from the given [`BinaryPackageCommand`].
    ///
    /// This is very cheap, as the hash is already available in the command.
    pub fn from_command(command: &BinaryPackageCommand) -> Self {
        Self {
            hash: command.hash().clone(),
            wasm: command.atom(),
        }
    }

    /// Get the module hash.
    pub fn hash(&self) -> &ModuleHash {
        &self.hash
    }

    /// Get the WASM code.
    pub fn wasm(&self) -> &OwnedBuffer {
        &self.wasm
    }

    pub fn into_parts(self) -> (ModuleHash, OwnedBuffer) {
        (self.hash, self.wasm)
    }
}
