use libc::c_int;
use wasmer_runtime_core::vm::Ctx;

// NOTE: Not implemented by Emscripten
pub fn ___lock(_ctx: &mut Ctx, what: c_int) {
    debug!("emscripten::___lock {}", what);
}

// NOTE: Not implemented by Emscripten
pub fn ___unlock(_ctx: &mut Ctx, what: c_int) {
    debug!("emscripten::___unlock {}", what);
}

// NOTE: Not implemented by Emscripten
pub fn ___wait(_ctx: &mut Ctx, _which: u32, _varargs: u32, _three: u32, _four: u32) {
    debug!("emscripten::___wait");
}
