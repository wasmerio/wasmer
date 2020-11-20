use crate::EmEnv;

// TODO: Need to implement.

/// emscripten: dlopen(filename: *const c_char, flag: c_int) -> *mut c_void
pub fn _dlopen(_ctx: &EmEnv, _filename: u32, _flag: u32) -> i32 {
    debug!("emscripten::_dlopen");
    -1
}

/// emscripten: dlclose(handle: *mut c_void) -> c_int
pub fn _dlclose(_ctx: &EmEnv, _filename: u32) -> i32 {
    debug!("emscripten::_dlclose");
    -1
}

/// emscripten: dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void
pub fn _dlsym(_ctx: &EmEnv, _filepath: u32, _symbol: u32) -> i32 {
    debug!("emscripten::_dlsym");
    -1
}

/// emscripten: dlerror() -> *mut c_char
pub fn _dlerror(_ctx: &EmEnv) -> i32 {
    debug!("emscripten::_dlerror");
    -1
}
