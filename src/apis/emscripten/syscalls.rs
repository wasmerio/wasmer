/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{c_int, c_void, ssize_t, write};

use crate::webassembly::Instance;

// A macro to retrieve variadic arguments given a varargs offset
macro_rules! vararg {
    ($name:ident, $type:ident, $instance:ident, $varargs:ident) => (
        let ($name, $varargs) = unsafe {
            use std::ptr;
            let ptr = $instance.memory_offset_addr(0, $varargs as usize);
            let ret = ptr::read(ptr as *const $type);
            (ret, $varargs + 4)
        };
    )
}

/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, _varargs: c_int, _instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    0
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall4");
    vararg!(fd, i32, instance, varargs);
    vararg!(buf_ptr, u32, instance, varargs);
    vararg!(count, u32, instance, varargs);
    debug!("fd: {}, buf_ptr: {}, count: {}", fd, buf_ptr, count);
    let buf = instance.memory_offset_addr(0, buf_ptr as usize) as *const c_void;
    unsafe { write(fd, buf, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    vararg!(pathname, u32, instance, varargs);
    vararg!(flags, u32, instance, varargs);
    vararg!(mode, u32, instance, varargs);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    -2
}

// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    vararg!(stream, u32, instance, varargs);
    vararg!(op, u32, instance, varargs);
    debug!("stream: {}, op: {}", stream, op);
    0
}

// sys_newuname
pub extern "C" fn ___syscall122(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    vararg!(buf, u32, instance, varargs);
    debug!("buf: {}", buf);
    0
}
