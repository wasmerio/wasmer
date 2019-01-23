use libc::c_int;
use wasmer_runtime_core::vm::Ctx;

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___lock(which: c_int, varargs: c_int, _vmctx: &mut Ctx) {
    debug!("emscripten::___lock {}, {}", which, varargs);
}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___unlock(which: c_int, varargs: c_int, _vmctx: &mut Ctx) {
    debug!("emscripten::___unlock {}, {}", which, varargs);
}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___wait(_which: c_int, _varargs: c_int, _vmctx: &mut Ctx) {
    debug!("emscripten::___wait");
}
