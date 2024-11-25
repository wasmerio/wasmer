use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_types::DeserializeError;

#[cfg(feature = "sys")]
use wasmer_compiler::Artifact;

use crate::{
    macros::rt::{gen_rt_ty, match_rt},
    IntoBytes, Store,
};

gen_rt_ty!(Engine @derives Debug, Clone);

impl RuntimeEngine {
    /// Returns the deterministic id of this engine.
    pub fn deterministic_id(&self) -> &str {
        match_rt!(on self  => s {
            s.deterministic_id()
        })
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
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
    pub(crate) unsafe fn deserialize_unchecked(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.deserialize_unchecked(bytes.into_bytes().to_owned().into()),
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `deserialize_unchecked`".into(),
            )),
        }
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Deserializes a WebAssembly module which was previously serialized with
    /// `Module::serialize`,
    ///
    /// # Errors
    /// Not every implementer supports serializing and deserializing modules.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    pub(crate) unsafe fn deserialize(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.deserialize(bytes.into_bytes().to_owned().into()),
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `deserialize`".into(),
            )),
        }
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
    pub(crate) unsafe fn deserialize_from_file_unchecked(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.deserialize_from_file_unchecked(file_ref),
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `deserialize_from_file_unchecked`".into(),
            )),
        }
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
    pub(crate) unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.deserialize_from_file(file_ref),
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `deserialize_from_file`".into(),
            )),
        }
    }
}

impl Default for RuntimeEngine {
    fn default() -> Self {
        #[cfg(feature = "sys")]
        {
            return Self::Sys(crate::rt::sys::entities::engine::default_engine());
        }

        #[cfg(feature = "wamr")]
        {
            return Self::Wamr(crate::rt::wamr::entities::engine::default_engine());
        }

        #[cfg(feature = "wasmi")]
        {
            return Self::Wasmi(crate::rt::wasmi::entities::engine::default_engine());
        }

        #[cfg(feature = "v8")]
        {
            return Self::V8(crate::rt::v8::entities::engine::default_engine());
        }

        #[cfg(feature = "js")]
        {
            return Self::Js(crate::rt::js::entities::engine::default_engine());
        }

        #[cfg(feature = "jsc")]
        {
            return Self::Jsc(crate::rt::jsc::entities::engine::default_engine());
        }

        panic!("No runtime enabled!")
    }
}
