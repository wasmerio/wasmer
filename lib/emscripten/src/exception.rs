use super::env;
use super::process::_abort;
use wasmer_runtime_core::{Instance, vm::Ctx};

/// emscripten: ___cxa_allocate_exception
pub extern "C" fn ___cxa_allocate_exception(size: u32, vmctx: &mut Ctx) -> u32 {
    debug!("emscripten::___cxa_allocate_exception");
    env::call_malloc(size as _, vmctx)
}

/// emscripten: ___cxa_throw
/// TODO: We don't have support for exceptions yet
pub extern "C" fn ___cxa_throw(_ptr: u32, ty: u32, destructor: u32, vmctx: &mut Ctx) {
    debug!("emscripten::___cxa_throw");
    _abort();
}
