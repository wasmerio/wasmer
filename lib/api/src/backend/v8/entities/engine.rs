//! Data types, functions and traits for `sys` runtime's `Engine` implementation.
use crate::{
    backend::v8::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t},
    BackendEngine,
};
use std::sync::Arc;

// A handle to an engine, which we want to unsafely mark as Sync.
struct EngineCapsule(*mut wasm_engine_t);

impl Drop for EngineCapsule {
    fn drop(&mut self) {
        unsafe { wasm_engine_delete(self.0) }
    }
}

unsafe impl Sync for EngineCapsule {}
unsafe impl Send for EngineCapsule {}

static ENGINE: std::sync::OnceLock<std::sync::Mutex<EngineCapsule>> = std::sync::OnceLock::new();

#[derive(Debug)]
pub(crate) struct CApiEngine {
    pub(crate) engine: *mut wasm_engine_t,
}

impl Default for CApiEngine {
    fn default() -> Self {
        let engine = ENGINE
            .get_or_init(|| unsafe { std::sync::Mutex::new(EngineCapsule(wasm_engine_new())) });
        let engine = unsafe { engine.lock().unwrap().0 };
        Self { engine }
    }
}

impl Drop for CApiEngine {
    fn drop(&mut self) {}
}

/// The V8 engine.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    pub(crate) inner: Arc<CApiEngine>,
}

impl Engine {
    /// Create a new instance of the `V8` engine.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn deterministic_id(&self) -> &str {
        "v8"
    }
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::backend::v8::engine::Engine`].
    pub fn into_v8(self) -> crate::backend::v8::engine::Engine {
        match self.be {
            BackendEngine::V8(s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::v8::engine::Engine`].
    pub fn as_v8(&self) -> &crate::backend::v8::engine::Engine {
        match &self.be {
            BackendEngine::V8(s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::v8::engine::Engine`].
    pub fn as_v8_mut(&mut self) -> &mut crate::backend::v8::engine::Engine {
        match self.be {
            BackendEngine::V8(ref mut s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `v8` runtime.
    pub fn is_v8(&self) -> bool {
        matches!(self.be, BackendEngine::V8(_))
    }
}

impl From<Engine> for crate::Engine {
    fn from(value: Engine) -> Self {
        crate::Engine {
            be: BackendEngine::V8(value),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
