use wasmer_runtime_core::{module::Module, vm::Ctx};

// Cargo culting this from our emscripten implementation for now, but it seems like a
// good thing to check; TODO: verify this is useful
pub fn is_wasi_module(module: &Module) -> bool {
    true
    // TODO:
}
