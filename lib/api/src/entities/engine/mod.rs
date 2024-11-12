//! Defines the [`Engine`] type, the [`EngineLike`] trait for implementors and useful
//! traits and data types to interact with them.

use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_compiler::Artifact;
use wasmer_types::DeserializeError;

use crate::{store::StoreLike, IntoBytes, ModuleCreator};

/// Create temporary handles to engines.
mod engine_ref;

pub use engine_ref::*;

/// The [`Engine`] is the entrypoint type for the runtime. It defines the kind of steps the runtime must take to execute
/// the WebAssembly module (compile, interpret..) and the place of execution (in-browser, host,
/// ..). This type is created from instances of [`EngineLike`] implementers.
// [todo] xdoardo: list the implementers in the docs above.
#[derive(Debug)]
pub struct Engine(pub(crate) Box<dyn EngineLike>);

impl Engine {
    /// Returns the deterministic id of this engine.
    fn deterministic_id(&self) -> &str {
        self.0.deterministic_id()
    }

    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Note
    /// You should almost always prefer [`EngineLike::deserialize`].
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
        EngineLike::deserialize_unchecked(self.0.as_ref(), bytes.into_bytes().into())
    }

    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    unsafe fn deserialize(&self, bytes: impl IntoBytes) -> Result<Arc<Artifact>, DeserializeError> {
        EngineLike::deserialize(self.0.as_ref(), bytes.into_bytes().into())
    }

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
        EngineLike::deserialize_from_file_unchecked(self.0.as_ref(), file_ref)
    }

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
        EngineLike::deserialize_from_file(self.0.as_ref(), file_ref)
    }

    /// Consume [`self`] and create the default [`StoreLike`] implementer for this engine.
    pub fn default_store(self) -> Box<dyn StoreLike> {
        todo!()
    }
}

impl Clone for Engine {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl Default for Engine {
    fn default() -> Self {
        todo!()
    }
}

/// The trait that every concrete engine must implement.
pub trait EngineLike: std::fmt::Debug + ModuleCreator {
    /// Returns the deterministic id of this engine.
    fn deterministic_id(&self) -> &str;

    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Note
    /// You should almost always prefer [`EngineLike::deserialize`].
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
        bytes: Bytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        _ = bytes;
        Err(DeserializeError::Generic(format!(
            "{} does not support serializing and deserializing modules",
            self.deterministic_id()
        )))
    }

    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    unsafe fn deserialize(&self, bytes: Bytes) -> Result<Arc<Artifact>, DeserializeError> {
        _ = bytes;
        Err(DeserializeError::Generic(format!(
            "{} does not support serializing and deserializing modules",
            self.deterministic_id()
        )))
    }

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
        _ = file_ref;
        Err(DeserializeError::Generic(format!(
            "{} does not support serializing and deserializing modules",
            self.deterministic_id()
        )))
    }

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
        _ = file_ref;
        Err(DeserializeError::Generic(format!(
            "{} does not support serializing and deserializing modules",
            self.deterministic_id()
        )))
    }

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn EngineLike>;
}
