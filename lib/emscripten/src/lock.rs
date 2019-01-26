use libc::c_int;
use wasmer_runtime_core::vm::Ctx;

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___lock(what: c_int, _ctx: &mut Ctx) {
    debug!("emscripten::___lock {}", what);
}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___unlock(what: c_int, _ctx: &mut Ctx) {
    debug!("emscripten::___unlock {}", what);
}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___wait(_which: c_int, _varargs: c_int, three: u32, four: u32, _ctx: &mut Ctx) {
    debug!("emscripten::___wait");
}
