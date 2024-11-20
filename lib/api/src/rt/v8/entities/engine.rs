//! Data types, functions and traits for `sys` runtime's `Engine` implementation.
use crate::{
    rt::v8::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t},
    RuntimeEngine,
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
    fn drop(&mut self) {
        unsafe { wasm_engine_delete(self.engine) }
    }
}

/// The V8 engine.
#[derive(Clone, Debug, Default)]
pub struct V8 {
    pub(crate) inner: Arc<CApiEngine>,
}

impl V8 {
    /// Create a new instance of the `V8` engine.
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn deterministic_id(&self) -> &str {
        "v8"
    }
}

unsafe impl Send for V8 {}
unsafe impl Sync for V8 {}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> V8 {
    V8::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::rt::v8::engine::Engine`].
    pub fn into_v8(self) -> crate::rt::v8::engine::V8 {
        match self.0 {
            RuntimeEngine::V8(s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::v8::engine::Engine`].
    pub fn as_v8(&self) -> &crate::rt::v8::engine::V8 {
        match &self.0 {
            RuntimeEngine::V8(s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::v8::engine::Engine`].
    pub fn as_v8_mut(&mut self) -> &mut crate::rt::v8::engine::V8 {
        match self.0 {
            RuntimeEngine::V8(ref mut s) => s,
            _ => panic!("Not a `v8` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `v8` runtime.
    pub fn is_v8(&self) -> bool {
        matches!(self.0, RuntimeEngine::V8(_))
    }
}

impl From<V8> for crate::Engine {
    fn from(value: V8) -> Self {
        Self(RuntimeEngine::V8(value))
    }
}
