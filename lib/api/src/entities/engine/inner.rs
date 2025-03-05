use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_types::{DeserializeError, Features};

#[cfg(feature = "sys")]
use wasmer_compiler::{types::target::Target, Artifact, CompilerConfig};

use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    BackendKind, IntoBytes, Store,
};

gen_rt_ty!(Engine @derives Debug, Clone);

impl BackendEngine {
    /// Returns the [`crate::BackendKind`] kind this engine belongs to.
    #[inline]
    pub fn get_be_kind(&self) -> crate::BackendKind {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(_) => crate::BackendKind::Sys,
            #[cfg(feature = "v8")]
            Self::V8(_) => crate::BackendKind::V8,
            #[cfg(feature = "wamr")]
            Self::Wamr(_) => crate::BackendKind::Wamr,
            #[cfg(feature = "wasmi")]
            Self::Wasmi(_) => crate::BackendKind::Wasmi,
            #[cfg(feature = "js")]
            Self::Js(_) => crate::BackendKind::Js,
            #[cfg(feature = "jsc")]
            Self::Jsc(_) => crate::BackendKind::Jsc,
        }
    }

    /// Returns the deterministic id of this engine.
    #[inline]
    pub fn deterministic_id(&self) -> &str {
        match_rt!(on self  => s {
            s.deterministic_id()
        })
    }

    /// Returns the default WebAssembly features that this backend enables for the given target.
    #[inline]
    pub fn default_features_for_target(&self, target: &Target) -> Features {
        // For backends, use the appropriate default features
        match self.get_be_kind() {
            #[cfg(feature = "sys")]
            crate::BackendKind::Sys => {
                // For SYS, we use target features from the engine
                // We already know this is a Sys backend due to the match branch
                let engine = match self {
                    Self::Sys(engine) => engine,
                    // This is unreachable because we're already in the Sys match branch
                    _ => unreachable!(),
                };

                // Try to access the inner features
                #[cfg(feature = "compiler")]
                {
                    let inner = engine.inner();
                    let features = inner.features();
                    return features.clone();
                }

                // Otherwise, use compiler defaults if available
                #[cfg(feature = "cranelift")]
                {
                    let cranelift_config = wasmer_compiler_cranelift::Cranelift::new();
                    return cranelift_config.default_features_for_target(target);
                }

                #[cfg(all(feature = "llvm", not(feature = "cranelift")))]
                {
                    let llvm_config = wasmer_compiler_llvm::LLVM::new();
                    return llvm_config.default_features_for_target(target);
                }

                #[cfg(all(
                    feature = "singlepass",
                    not(feature = "cranelift"),
                    not(feature = "llvm")
                ))]
                {
                    let singlepass_config = wasmer_compiler_singlepass::Singlepass::new();
                    return singlepass_config.default_features_for_target(target);
                }

                // Fallback to default
                Features::default()
            }

            // For other backends, provide hardcoded defaults
            _ => Features::default(),
        }
    }

    /// Returns all WebAssembly features that this backend is capable of supporting for the given target.
    #[inline]
    pub fn supported_features_for_target(backend: &BackendKind, target: &Target) -> Features {
        // For backends, use the appropriate supported features
        match backend {
            #[cfg(feature = "sys")]
            crate::BackendKind::Sys => {
                // Call the dedicated function in the sys engine module
                crate::backend::sys::entities::engine::supported_features(target)
            }

            #[cfg(feature = "v8")]
            crate::BackendKind::V8 => {
                // Get V8-specific features
                crate::backend::v8::engine::Engine::supported_features()
            }

            #[cfg(feature = "wasmi")]
            crate::BackendKind::Wasmi => {
                // Get WASMI-specific features
                crate::backend::wasmi::engine::Engine::supported_features()
            }

            // Default features for all other backends
            _ => Features::default(),
        }
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
    #[inline]
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
    #[inline]
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
    #[inline]
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
