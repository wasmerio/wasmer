//! Data types, functions and traits for `wasmi` runtime's `Engine` implementation.
use crate::{
    backend::wasmi::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t},
    BackendEngine,
};
use std::sync::Arc;
use wasmer_types::{target::Target, Features};

#[derive(Debug)]
pub(crate) struct CApiEngine {
    pub(crate) engine: *mut wasm_engine_t,
}

impl Default for CApiEngine {
    fn default() -> Self {
        let engine: *mut wasm_engine_t = unsafe { wasm_engine_new() };
        Self { engine }
    }
}

impl Drop for CApiEngine {
    fn drop(&mut self) {
        unsafe { wasm_engine_delete(self.engine) }
    }
}

/// The engine for the Web Assembly Micro Runtime.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    pub(crate) inner: Arc<CApiEngine>,
}

impl Engine {
    /// Create a new instance of the `wasmi` engine.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn deterministic_id(&self) -> String {
        String::from("wasmi")
    }

    /// Returns the WebAssembly features supported by the WASMI engine.
    pub fn supported_features() -> Features {
        // WASMI-specific features
        let mut features = Features::default();
        features.bulk_memory(true);
        features.reference_types(true);
        features.multi_value(true);
        features.simd(false);
        features.threads(false);
        features.exceptions(false);
        features
    }

    /// Returns the default features for the WASMI engine.
    pub fn default_features() -> Features {
        Self::supported_features()
    }
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// Returns the default engine for the wasmi engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::backend::wasmi::engine::Engine`].
    pub fn into_wasmi(self) -> crate::backend::wasmi::engine::Engine {
        match self.be {
            BackendEngine::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::wasmi::engine::Engine`].
    pub fn as_wasmi(&self) -> &crate::backend::wasmi::engine::Engine {
        match &self.be {
            BackendEngine::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::wasmi::engine::Engine`].
    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::engine::Engine {
        match self.be {
            BackendEngine::Wasmi(ref mut s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `wasmi` runtime.
    pub fn is_wasmi(&self) -> bool {
        matches!(self.be, BackendEngine::Wasmi(_))
    }
}

impl From<Engine> for crate::Engine {
    fn from(value: Engine) -> Self {
        crate::Engine {
            be: BackendEngine::Wasmi(value),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
