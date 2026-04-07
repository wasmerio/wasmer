//! Data types, functions and traits for `wasmi` runtime's `Engine` implementation.
use crate::BackendEngine;
use ::wasmi;
use std::sync::Arc;
use wasmer_types::{Features, target::Target};

#[derive(Debug)]
pub(crate) struct NativeEngine {
    pub(crate) engine: wasmi::Engine,
}

impl Default for NativeEngine {
    fn default() -> Self {
        let engine = wasmi::Engine::default();
        Self { engine }
    }
}

/// The engine for the Web Assembly Micro Runtime.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    pub(crate) inner: Arc<NativeEngine>,
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
        features.simd(true);
        features.threads(true);
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
        match &mut self.be {
            BackendEngine::Wasmi(s) => s,
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
        Self {
            be: BackendEngine::Wasmi(value),
            id: Self::atomic_next_engine_id(),
        }
    }
}
