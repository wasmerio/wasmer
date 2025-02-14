//! Defines the [`self::Engine`] type and useful traits and data types to interact with an engine.

use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_types::DeserializeError;

#[cfg(feature = "sys")]
use wasmer_compiler::Artifact;

use crate::{IntoBytes, Store};

/// Create temporary handles to engines.
mod engine_ref;

/// The actual (private) definition of the engines.
mod inner;
pub(crate) use inner::BackendEngine;

pub use engine_ref::*;

/// An engine identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct EngineId(u64);

/// The [`Engine`] is the entrypoint type for the runtime. It defines the kind of steps the runtime must take to execute
/// the WebAssembly module (compile, interpret..) and the place of execution (in-browser, host,
/// ..).
#[derive(Debug, Clone)]
pub struct Engine {
    pub(crate) be: BackendEngine,
    pub(crate) id: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            be: Default::default(),
            id: Self::atomic_next_engine_id(),
        }
    }
}

impl Engine {
    pub(crate) fn atomic_next_engine_id() -> u64 {
        static ENGINE_ID_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        ENGINE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Returns the [`crate::BackendKind`] kind this engine belongs to.
    pub fn get_backend_kind(&self) -> crate::BackendKind {
        self.be.get_be_kind()
    }

    /// Returns the deterministic id of this engine.
    pub fn deterministic_id(&self) -> &str {
        self.be.deterministic_id()
    }

    /// Returns the unique id of this engine.
    pub fn id(&self) -> EngineId {
        EngineId(self.id)
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Note
    /// You should almost always prefer [`Self::deserialize`].
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    ///
    /// # Safety
    /// See [`Artifact::deserialize_unchecked`].
    unsafe fn deserialize_unchecked(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        self.be.deserialize_unchecked(bytes)
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    unsafe fn deserialize(&self, bytes: impl IntoBytes) -> Result<Arc<Artifact>, DeserializeError> {
        self.be.deserialize(bytes)
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Load a serialized WebAssembly module from a file and deserialize it.
    ///
    /// # Note
    /// You should almost always prefer [`Self::deserialize_from_file`].
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    ///
    /// # Safety
    /// See [`Artifact::deserialize_unchecked`].
    unsafe fn deserialize_from_file_unchecked(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        self.be.deserialize_from_file_unchecked(file_ref)
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Load a serialized WebAssembly module from a file and deserialize it.
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    ///
    /// # Safety
    /// See [`Artifact::deserialize`].
    unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        self.be.deserialize_from_file_unchecked(file_ref)
    }
}
