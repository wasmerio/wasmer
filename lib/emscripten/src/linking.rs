use wasmer_runtime_core::vm::Ctx;

// TODO: Need to implement.

/// emscripten: dlopen(filename: *const c_char, flag: c_int) -> *mut c_void
pub fn _dlopen(_ctx: &mut Ctx, _filename: u32, _flag: u32) -> i32 {
    debug!("emscripten::_dlopen");
    -1
}

/// emscripten: dlclose(handle: *mut c_void) -> c_int
pub fn _dlclose(_ctx: &mut Ctx, _filename: u32) -> i32 {
    debug!("emscripten::_dlclose");
    -1
}

/// emscripten: dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void
pub fn _dlsym(_ctx: &mut Ctx, _filepath: u32, _symbol: u32) -> i32 {
    debug!("emscripten::_dlsym");
    -1
}

/// emscripten: dlerror() -> *mut c_char
pub fn _dlerror(_ctx: &mut Ctx) -> i32 {
    debug!("emscripten::_dlerror");
    -1
}
