/// A WebAssembly `Universal` Engine.
#[derive(Clone)]
pub struct Engine;

impl Default for Engine {
    fn default() -> Self {
        Engine
    }
}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}
