use super::bindings::{wasm_engine_delete, wasm_engine_new, wasm_engine_t};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct CApiEngine {
    pub(crate) engine: *mut wasm_engine_t,
}

#[cfg(feature = "v8")]
// A handle to an engine, which we want to unsafely mark as Sync.
struct EngineCapsule(*mut wasm_engine_t);

#[cfg(feature = "v8")]
impl Drop for EngineCapsule {
    fn drop(&mut self) {
        unsafe { wasm_engine_delete(self.0) }
    }
}

#[cfg(feature = "v8")]
unsafe impl Sync for EngineCapsule {}

#[cfg(feature = "v8")]
unsafe impl Send for EngineCapsule {}

#[cfg(feature = "v8")]
static ENGINE: std::sync::OnceLock<std::sync::Mutex<EngineCapsule>> = std::sync::OnceLock::new();

impl Default for CApiEngine {
    #[cfg(not(feature = "v8"))]
    fn default() -> Self {
        let engine: *mut wasm_engine_t = unsafe { wasm_engine_new() };
        Self { engine }
    }

    #[cfg(feature = "v8")]
    fn default() -> Self {
        let engine = ENGINE
            .get_or_init(|| unsafe { std::sync::Mutex::new(EngineCapsule(wasm_engine_new())) });
        let engine = unsafe { engine.lock().unwrap().0 };
        Self { engine }
    }
}

impl Drop for CApiEngine {
    #[cfg(feature = "v8")]
    fn drop(&mut self) {}

    #[cfg(not(feature = "v8"))]
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
