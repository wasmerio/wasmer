use super::env;
use super::process::_abort;
use wasmer_runtime_core::vm::Ctx;

/// emscripten: ___cxa_allocate_exception
pub fn ___cxa_allocate_exception(size: u32, ctx: &mut Ctx) -> u32 {
    debug!("emscripten::___cxa_allocate_exception");
    env::call_malloc(size as _, ctx)
}

/// emscripten: ___cxa_throw
/// TODO: We don't have support for exceptions yet
pub fn ___cxa_throw(_ptr: u32, _ty: u32, _destructor: u32, ctx: &mut Ctx) {
    debug!("emscripten::___cxa_throw");
    _abort(ctx);
}
