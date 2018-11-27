use crate::webassembly::Instance;
use libc::c_int;

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___lock(_which: c_int, _varargs: c_int, _instance: &mut Instance) {}

// NOTE: Not implemented by Emscripten
pub extern "C" fn ___unlock(_which: c_int, _varargs: c_int, _instance: &mut Instance) {}
