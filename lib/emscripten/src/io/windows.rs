use crate::EmEnv;
use wasmer::FunctionEnvMut;

// This may be problematic for msvc which uses inline functions for the printf family
// this cfg_attr will try to link with the legacy lib that does not inline printf
// this will allow for compiliation, but will produce a linker error if there is a problem
// finding printf.
//#[cfg_attr(
//    all(windows, target_env = "msvc"),
//    link(name = "legacy_stdio_definitions", kind = "static-nobundle")
//)]
//extern "C" {
//    #[link_name = "printf"]
//    pub fn _printf(s: *const c_char, ...) -> c_int;
//}

/// putchar
pub fn putchar(_ctx: FunctionEnvMut<EmEnv>, chr: i32) {
    unsafe { libc::putchar(chr) };
}

/// printf
pub fn printf(_ctx: FunctionEnvMut<EmEnv>, memory_offset: i32, extra: i32) -> i32 {
    debug!("emscripten::printf {}, {}", memory_offset, extra);
    #[cfg(not(feature = "debug"))]
    {
        let _ = memory_offset;
        let _ = extra;
    }
    //    unsafe {
    //        let addr = emscripten_memory_pointer!(ctx.memory(0), memory_offset) as _;
    //        _printf(addr, extra)
    //    }
    -1
}

/// chroot
pub fn chroot(_ctx: FunctionEnvMut<EmEnv>, _name_ptr: i32) -> i32 {
    debug!("emscripten::chroot");
    unimplemented!("emscripten::chroot")
}

/// getpwuid
pub fn getpwuid(_ctx: FunctionEnvMut<EmEnv>, _uid: i32) -> i32 {
    debug!("emscripten::getpwuid");
    unimplemented!("emscripten::getpwuid")
}
