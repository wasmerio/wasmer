use crate::webassembly::Instance;
use libc::{c_int, c_void};
use std::cell::UnsafeCell;

/// setjmp
pub extern "C" fn __setjmp(env_addr: u32, instance: &mut Instance) -> c_int {
    debug!("emscripten::__setjmp (setjmp)");
    unsafe {
        // Rather than using the env as the holder of the jump address,
        // we obscure that id so we are in complete control of it
        let obscure_env = instance.memory_offset_addr(0, env_addr as usize) as *mut i8;
        let jmp_buf: UnsafeCell<[c_int; 27]> = UnsafeCell::new([0; 27]);
        let mut jumps = &mut instance.emscripten_data.as_mut().unwrap().jumps;
        let result = setjmp(jmp_buf.get() as _);
        *obscure_env = jumps.len() as _;
        jumps.push(jmp_buf);
        // We use the index of the jump as the jump buffer (env)
        result
    }
}

/// longjmp
pub extern "C" fn __longjmp(env_addr: u32, val: c_int, instance: &mut Instance) -> ! {
    debug!("emscripten::__longjmp (longjmp) {}", val);
    unsafe {
        let obscure_env = instance.memory_offset_addr(0, env_addr as usize) as *mut i8;
        let mut jumps = &mut instance.emscripten_data.as_mut().unwrap().jumps;
        let mut real_env = &jumps[*obscure_env as usize];
        longjmp(real_env.get() as _, val)
    };
}

extern "C" {
    fn setjmp(env: *mut c_void) -> c_int;
    fn longjmp(env: *mut c_void, val: c_int) -> !;
}
