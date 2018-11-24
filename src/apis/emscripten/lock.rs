use libc::c_int;
use crate::webassembly::Instance;

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___lock(_which: c_int, _varargs: c_int, _instance: &mut Instance) {}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___unlock(_which: c_int, _varargs: c_int, _instance: &mut Instance) {}
