//! The cache module provides the common data structures used by compiler backends to allow
//! serializing compiled wasm code to a binary format.  The binary format can be persisted,
//! and loaded to allow skipping compilation and fast startup.

use crate::hash::Hash;
use std::error::Error;
use wasmer::{AsEngineRef, Module};

/// A generic cache for storing and loading compiled wasm modules.
pub trait Cache {
    /// The serialization error for the implementation
    type SerializeError: Error + Send + Sync;
    /// The deserialization error for the implementation
    type DeserializeError: Error + Send + Sync;

    /// Loads a module using the provided [`wasmer::Store`] and [`crate::Hash`].
    ///
    /// # Safety
    /// This function is unsafe as the cache store could be tampered with.
    unsafe fn load(
        &self,
        engine: &impl AsEngineRef,
        key: Hash,
    ) -> Result<Module, Self::DeserializeError>;

    /// Store a [`Module`] into the cache with the given [`crate::Hash`].
    fn store(&mut self, key: Hash, module: &Module) -> Result<(), Self::SerializeError>;
}
