use crate::EmEnv;
use wasmer::FunctionEnvMut;

// TODO: Need to implement.

/// emscripten: dlopen(filename: *const c_char, flag: c_int) -> *mut c_void
pub fn _dlopen(mut _ctx: FunctionEnvMut<EmEnv>, _filename: u32, _flag: u32) -> i32 {
    debug!("emscripten::_dlopen");
    -1
}

/// emscripten: dlclose(handle: *mut c_void) -> c_int
pub fn _dlclose(mut _ctx: FunctionEnvMut<EmEnv>, _filename: u32) -> i32 {
    debug!("emscripten::_dlclose");
    -1
}

/// emscripten: dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void
pub fn _dlsym(mut _ctx: FunctionEnvMut<EmEnv>, _filepath: u32, _symbol: u32) -> i32 {
    debug!("emscripten::_dlsym");
    -1
}

/// emscripten: dlerror() -> *mut c_char
pub fn _dlerror(mut _ctx: FunctionEnvMut<EmEnv>) -> i32 {
    debug!("emscripten::_dlerror");
    -1
}
