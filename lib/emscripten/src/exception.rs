use super::env;
use super::process::_abort;
use wasmer_runtime_core::vm::Ctx;

/// emscripten: ___cxa_allocate_exception
pub fn ___cxa_allocate_exception(ctx: &mut Ctx, size: u32) -> u32 {
    debug!("emscripten::___cxa_allocate_exception");
    env::call_malloc(ctx, size as _)
}

/// emscripten: ___cxa_throw
/// TODO: We don't have support for exceptions yet
pub fn ___cxa_throw(ctx: &mut Ctx, _ptr: u32, _ty: u32, _destructor: u32) {
    debug!("emscripten::___cxa_throw");
    _abort(ctx);
}
