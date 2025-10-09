use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_types::{DeserializeError, Features, target::Target};

#[cfg(feature = "sys")]
use wasmer_compiler::Artifact;
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;

use crate::{
    BackendKind, IntoBytes, Store,
    macros::backend::{gen_rt_ty, match_rt},
};

gen_rt_ty!(Engine @derives Debug, Clone);

impl BackendEngine {
    /// Returns the deterministic id of this engine.
    #[inline]
    pub fn deterministic_id(&self) -> String {
        match_rt!(on self  => s {
            s.deterministic_id()
        })
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
    #[inline]
    pub(crate) unsafe fn deserialize_unchecked(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => unsafe {
                s.deserialize_unchecked(bytes.into_bytes().to_owned().into())
            },
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
    #[inline]
    pub(crate) unsafe fn deserialize(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => unsafe { s.deserialize(bytes.into_bytes().to_owned().into()) },
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
    #[inline]
    pub(crate) unsafe fn deserialize_from_file_unchecked(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => unsafe { s.deserialize_from_file_unchecked(file_ref) },
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
    #[inline]
    pub(crate) unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => unsafe { s.deserialize_from_file(file_ref) },
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `deserialize_from_file`".into(),
            )),
        }
    }
}

impl Default for BackendEngine {
    #[allow(unreachable_code)]
    #[inline]
    fn default() -> Self {
        #[cfg(feature = "sys-default")]
        {
            return Self::Sys(crate::backend::sys::entities::engine::default_engine());
        }

        #[cfg(feature = "wamr-default")]
        {
            return Self::Wamr(crate::backend::wamr::entities::engine::default_engine());
        }

        #[cfg(feature = "wasmi-default")]
        {
            return Self::Wasmi(crate::backend::wasmi::entities::engine::default_engine());
        }

        #[cfg(feature = "v8-default")]
        {
            return Self::V8(crate::backend::v8::entities::engine::default_engine());
        }

        #[cfg(feature = "js-default")]
        {
            return Self::Js(crate::backend::js::entities::engine::default_engine());
        }

        #[cfg(feature = "jsc-default")]
        {
            return Self::Jsc(crate::backend::jsc::entities::engine::default_engine());
        }

        #[cfg(feature = "sys")]
        {
            return Self::Sys(crate::backend::sys::entities::engine::default_engine());
        }

        #[cfg(feature = "wamr")]
        {
            return Self::Wamr(crate::backend::wamr::entities::engine::default_engine());
        }

        #[cfg(feature = "wasmi")]
        {
            return Self::Wasmi(crate::backend::wasmi::entities::engine::default_engine());
        }

        #[cfg(feature = "v8")]
        {
            return Self::V8(crate::backend::v8::entities::engine::default_engine());
        }

        #[cfg(feature = "js")]
        {
            return Self::Js(crate::backend::js::entities::engine::default_engine());
        }

        #[cfg(feature = "jsc")]
        {
            return Self::Jsc(crate::backend::jsc::entities::engine::default_engine());
        }

        panic!("No runtime enabled!")
    }
}
