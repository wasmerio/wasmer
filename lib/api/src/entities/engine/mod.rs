//! Defines the [`self::Engine`] type and useful traits and data types to interact with an engine.

use bytes::Bytes;
use std::{path::Path, sync::Arc};
use wasmer_types::{
    CompileError, DeserializeError, Features,
    target::{Target, UserCompilerOptimizations},
};

#[cfg(feature = "sys")]
use wasmer_compiler::Artifact;

#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;

use crate::{BackendKind, IntoBytes, Store};

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
        #[cfg(target_has_atomic = "64")]
        {
            static ENGINE_ID_COUNTER: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            return ENGINE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        #[allow(unreachable_code)]
        #[cfg(target_has_atomic = "32")]
        {
            static ENGINE_ID_COUNTER: std::sync::atomic::AtomicU32 =
                std::sync::atomic::AtomicU32::new(0);
            ENGINE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as _
        }
    }

    /// Returns the deterministic id of this engine.
    pub fn deterministic_id(&self) -> String {
        self.be.deterministic_id()
    }

    /// Returns the unique id of this engine.
    pub fn id(&self) -> EngineId {
        EngineId(self.id)
    }

    /// Returns the default WebAssembly features supported by this backend for a given target.
    ///
    /// These are the features that will be enabled by default without any user configuration.
    pub fn default_features_for_backend(backend: &crate::BackendKind, target: &Target) -> Features {
        match backend {
            #[cfg(feature = "cranelift")]
            crate::BackendKind::Cranelift => {
                wasmer_compiler_cranelift::Cranelift::default().default_features_for_target(target)
            }
            #[cfg(feature = "llvm")]
            crate::BackendKind::LLVM => {
                wasmer_compiler_llvm::LLVM::default().default_features_for_target(target)
            }
            #[cfg(feature = "singlepass")]
            crate::BackendKind::Singlepass => wasmer_compiler_singlepass::Singlepass::default()
                .default_features_for_target(target),
            #[cfg(feature = "v8")]
            crate::BackendKind::V8 => {
                // Get V8-specific features
                crate::backend::v8::engine::Engine::default_features()
            }
            #[cfg(feature = "wamr")]
            crate::BackendKind::Wamr => {
                // Get WAMR-specific features
                crate::backend::wamr::engine::Engine::default_features()
            }
            #[cfg(feature = "wasmi")]
            crate::BackendKind::Wasmi => {
                // Get WASMI-specific features
                crate::backend::wasmi::engine::Engine::default_features()
            }
            #[cfg(feature = "js")]
            crate::BackendKind::Js => {
                // Get JS-specific features
                crate::backend::js::engine::Engine::default_features()
            }
            #[cfg(feature = "jsc")]
            crate::BackendKind::Jsc => {
                // Get JSC-specific features
                crate::backend::jsc::engine::Engine::default_features()
            }
            // Default case
            _ => Features::default(),
        }
    }

    /// Returns all WebAssembly features supported by the specified backend for a given target.
    ///
    /// This static method allows checking features for any backend, not just the current one.
    pub fn supported_features_for_backend(
        backend: &crate::BackendKind,
        target: &Target,
    ) -> Features {
        match backend {
            #[cfg(feature = "cranelift")]
            crate::BackendKind::Cranelift => wasmer_compiler_cranelift::Cranelift::default()
                .supported_features_for_target(target),
            #[cfg(feature = "llvm")]
            crate::BackendKind::LLVM => {
                wasmer_compiler_llvm::LLVM::default().supported_features_for_target(target)
            }
            #[cfg(feature = "singlepass")]
            crate::BackendKind::Singlepass => wasmer_compiler_singlepass::Singlepass::default()
                .supported_features_for_target(target),
            #[cfg(feature = "v8")]
            crate::BackendKind::V8 => {
                // Get V8-specific features
                crate::backend::v8::engine::Engine::supported_features()
            }
            #[cfg(feature = "wamr")]
            crate::BackendKind::Wamr => {
                // Get WAMR-specific features
                crate::backend::wamr::engine::Engine::supported_features()
            }
            #[cfg(feature = "wasmi")]
            crate::BackendKind::Wasmi => {
                // Get WASMI-specific features
                crate::backend::wasmi::engine::Engine::supported_features()
            }
            #[cfg(feature = "js")]
            crate::BackendKind::Js => {
                // Get JS-specific features
                crate::backend::js::engine::Engine::supported_features()
            }
            #[cfg(feature = "jsc")]
            crate::BackendKind::Jsc => {
                // Get JSC-specific features
                crate::backend::jsc::engine::Engine::supported_features()
            }
            // Default case
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
    unsafe fn deserialize_unchecked(
        &self,
        bytes: impl IntoBytes,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        unsafe { self.be.deserialize_unchecked(bytes) }
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
        unsafe { self.be.deserialize(bytes) }
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
        unsafe { self.be.deserialize_from_file_unchecked(file_ref) }
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
        unsafe { self.be.deserialize_from_file_unchecked(file_ref) }
    }

    /// Add suggested optimizations to this engine.
    ///
    /// # Note
    ///
    /// Not every backend supports every optimization. This function may fail (i.e. not set the
    /// suggested optimizations) silently if the underlying engine backend does not support one or
    /// more optimizations.
    pub fn with_opts(
        &mut self,
        suggested_opts: &UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        match self.be {
            #[cfg(feature = "sys")]
            BackendEngine::Sys(ref mut e) => e.with_opts(suggested_opts),
            _ => Ok(()),
        }
    }

    #[cfg(feature = "experimental-async")]
    /// Returns true if the engine supports async operations.
    pub fn supports_async(&self) -> bool {
        match self.be {
            #[cfg(feature = "sys")]
            BackendEngine::Sys(ref e) => true,
            _ => false,
        }
    }
}
