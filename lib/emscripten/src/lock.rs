use crate::EmEnv;
use libc::c_int;

// NOTE: Not implemented by Emscripten
pub fn ___lock(_ctx: &EmEnv, _what: c_int) {
    debug!("emscripten::___lock {}", _what);
}

// NOTE: Not implemented by Emscripten
pub fn ___unlock(_ctx: &EmEnv, _what: c_int) {
    debug!("emscripten::___unlock {}", _what);
}

// NOTE: Not implemented by Emscripten
pub fn ___wait(_ctx: &EmEnv, _which: u32, _varargs: u32, _three: u32, _four: u32) {
    debug!("emscripten::___wait");
}

pub fn _flock(_ctx: &EmEnv, _fd: u32, _op: u32) -> u32 {
    debug!("emscripten::_flock");
    0
}
