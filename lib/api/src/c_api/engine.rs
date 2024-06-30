use super::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t};
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

/// A WebAssembly `Universal` Engine.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    pub(crate) inner: Arc<CApiEngine>,
}

impl Engine {
    pub(crate) fn deterministic_id(&self) -> &str {
        "wasm-c-api"
    }
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

impl From<&crate::engine::Engine> for Engine {
    fn from(engine: &crate::engine::Engine) -> Self {
        unimplemented!();
    }
}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}
