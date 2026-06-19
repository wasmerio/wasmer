use std::sync::Arc;
use wasmer_types::DeserializeError;

#[cfg(feature = "sys")]
use wasmer_compiler::Artifact;
#[cfg(feature = "compiler")]
use wasmer_compiler::CompilerConfig;

use crate::{
    BackendKind, Store,
    macros::backend::{gen_rt_ty, match_rt},
};

gen_rt_ty! {
    #[derive(Debug, Clone)]
    pub(crate) BackendEngine(entities::engine::Engine);
}

impl BackendEngine {
    /// Returns the deterministic id of this engine.
    #[inline]
    pub fn deterministic_id(&self) -> String {
        match_rt!(on self  => s {
            s.deterministic_id()
        })
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Load a serialized WebAssembly module from a file.
    ///
    /// # Errors
    /// Not every implementer supports loading modules from a file.
    /// Currently, only the `sys` engines support it, and only when the target
    /// architecture is not `wasm32`.
    #[inline]
    pub(crate) fn load_from_file(
        &self,
        file: std::fs::File,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.load_from_file(file),
            _ => Err(DeserializeError::Generic(
                "The selected runtime does not support `load_from_file`".into(),
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

        #[cfg(feature = "v8-default")]
        {
            return Self::V8(crate::backend::v8::entities::engine::default_engine());
        }

        #[cfg(feature = "js-default")]
        {
            return Self::Js(crate::backend::js::entities::engine::default_engine());
        }

        #[cfg(feature = "sys")]
        {
            return Self::Sys(crate::backend::sys::entities::engine::default_engine());
        }

        #[cfg(feature = "v8")]
        {
            return Self::V8(crate::backend::v8::entities::engine::default_engine());
        }

        #[cfg(feature = "js")]
        {
            return Self::Js(crate::backend::js::entities::engine::default_engine());
        }

        panic!("No runtime enabled!")
    }
}
