use crate::webassembly::Instance;
use libc::{c_int, c_void};
use std::cell::UnsafeCell;

/// setjmp
pub extern "C" fn __setjmp(env_addr: u32, instance: &mut Instance) -> c_int {
    debug!("emscripten::__setjmp (setjmp)");
    unsafe {
        // Rather than using the env as the holder of the jump buffer pointer,
        // we use the environment address to store the index relative to jumps
        // so the address of the jump it's outside the wasm memory itself.
        let jump_index = instance.memory_offset_addr(0, env_addr as usize) as *mut i8;
        // We create the jump buffer outside of the wasm memory
        let jump_buf: UnsafeCell<[c_int; 27]> = UnsafeCell::new([0; 27]);
        let mut jumps = &mut instance.emscripten_data.as_mut().unwrap().jumps;
        let result = setjmp(jump_buf.get() as _);
        // We set the jump index to be the last value of jumps
        *jump_index = jumps.len() as _;
        // We hold the reference of the jump buffer
        jumps.push(jump_buf);
        result
    }
}

/// longjmp
pub extern "C" fn __longjmp(env_addr: u32, val: c_int, instance: &mut Instance) -> ! {
    debug!("emscripten::__longjmp (longjmp) {}", val);
    unsafe {
        // We retrieve the jump index from the env address
        let jump_index = instance.memory_offset_addr(0, env_addr as usize) as *mut i8;
        let mut jumps = &mut instance.emscripten_data.as_mut().unwrap().jumps;
        // We get the real jump buffer from the jumps vector, using the retrieved index
        let mut jump_buf = &jumps[*jump_index as usize];
        longjmp(jump_buf.get() as _, val)
    };
}

extern "C" {
    fn setjmp(env: *mut c_void) -> c_int;
    fn longjmp(env: *mut c_void, val: c_int) -> !;
}
