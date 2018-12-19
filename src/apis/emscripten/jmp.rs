use libc::{c_void, c_int};
use crate::webassembly::Instance;


/// setjmp
pub extern "C" fn __setjmp(
    env_addr: u32,
    instance: &mut Instance,
) -> c_int {
    debug!("emscripten::__setjmp (setjmp)");
    unsafe {
        let env = instance.memory_offset_addr(0, env_addr as usize) as *mut c_void;
        setjmp(env)
    }
}


/// longjmp
pub extern "C" fn __longjmp(
    env_addr: u32,
    val: c_int,
    instance: &mut Instance,
) -> ! {
    debug!("emscripten::__longjmp (longjmp) {}", val);
    unsafe {
        let env = instance.memory_offset_addr(0, env_addr as usize) as *mut c_void;
        longjmp(env, val)
    };
}

extern "C" {
    fn setjmp(env: *mut c_void) -> c_int;
    fn longjmp(env: *mut c_void, val: c_int) -> !;
}
