use super::env;
use super::process::_abort;
use crate::EmEnv;
use wasmer::FunctionEnvMut;

/// emscripten: ___cxa_allocate_exception
pub fn ___cxa_allocate_exception(mut ctx: FunctionEnvMut<EmEnv>, size: u32) -> u32 {
    debug!("emscripten::___cxa_allocate_exception");
    env::call_malloc(&mut ctx.as_mut(), size as _)
}

pub fn ___cxa_current_primary_exception(_ctx: FunctionEnvMut<EmEnv>) -> u32 {
    debug!("emscripten::___cxa_current_primary_exception");
    unimplemented!("emscripten::___cxa_current_primary_exception")
}

pub fn ___cxa_decrement_exception_refcount(_ctx: FunctionEnvMut<EmEnv>, _a: u32) {
    debug!("emscripten::___cxa_decrement_exception_refcount({})", _a);
    unimplemented!("emscripten::___cxa_decrement_exception_refcount({})", _a)
}

pub fn ___cxa_increment_exception_refcount(_ctx: FunctionEnvMut<EmEnv>, _a: u32) {
    debug!("emscripten::___cxa_increment_exception_refcount({})", _a);
    unimplemented!("emscripten::___cxa_increment_exception_refcount({})", _a)
}

pub fn ___cxa_rethrow_primary_exception(_ctx: FunctionEnvMut<EmEnv>, _a: u32) {
    debug!("emscripten::___cxa_rethrow_primary_exception({})", _a);
    unimplemented!("emscripten::___cxa_rethrow_primary_exception({})", _a)
}

/// emscripten: ___cxa_throw
/// TODO: We don't have support for exceptions yet
pub fn ___cxa_throw(ctx: FunctionEnvMut<EmEnv>, _ptr: u32, _ty: u32, _destructor: u32) {
    debug!("emscripten::___cxa_throw");
    eprintln!("Throwing exceptions not yet implemented: aborting!");
    _abort(ctx);
}

pub fn ___cxa_begin_catch(_ctx: FunctionEnvMut<EmEnv>, _exception_object_ptr: u32) -> i32 {
    debug!("emscripten::___cxa_begin_catch");
    -1
}

pub fn ___cxa_end_catch(_ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::___cxa_end_catch");
}

pub fn ___cxa_uncaught_exception(_ctx: FunctionEnvMut<EmEnv>) -> i32 {
    debug!("emscripten::___cxa_uncaught_exception");
    -1
}

pub fn ___cxa_pure_virtual(_ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::___cxa_pure_virtual");
    // ABORT = true
    panic!("Pure virtual function called!");
}
