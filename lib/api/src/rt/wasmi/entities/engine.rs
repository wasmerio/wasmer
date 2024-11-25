//! Data types, functions and traits for `v8` runtime's `Engine` implementation.
use crate::{
    rt::wasmi::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t},
    RuntimeEngine,
};
use std::sync::Arc;

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

    pub(crate) fn deterministic_id(&self) -> &str {
        "wasmi"
    }
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::rt::wasmi::engine::Engine`].
    pub fn into_wasmi(self) -> crate::rt::wasmi::engine::Engine {
        match self.rt {
            RuntimeEngine::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::wasmi::engine::Engine`].
    pub fn as_wasmi(&self) -> &crate::rt::wasmi::engine::Engine {
        match &self.rt {
            RuntimeEngine::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::wasmi::engine::Engine`].
    pub fn as_wasmi_mut(&mut self) -> &mut crate::rt::wasmi::engine::Engine {
        match self.rt {
            RuntimeEngine::Wasmi(ref mut s) => s,
            _ => panic!("Not a `wasmi` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `wasmi` runtime.
    pub fn is_wasmi(&self) -> bool {
        matches!(self.rt, RuntimeEngine::Wasmi(_))
    }
}

impl From<Engine> for crate::Engine {
    fn from(value: Engine) -> Self {
        crate::Engine {
            rt: RuntimeEngine::Wasmi(value),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
