/// NOTE: TODO: These syscalls only support wasm_32 for now because they take u32 offset
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{c_int, c_void, ssize_t, write};
#[macro_use]
use macros;
use crate::webassembly::Instance;


/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, mut _varargs: c_int, _instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    0
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall4");
    let fd = vararg!(fd, i32, instance, varargs);
    let buf_ptr = vararg!(buf_ptr, u32, instance, varargs);
    let count = vararg!(count, u32, instance, varargs);
    debug!("fd: {}, buf_ptr: {}, count: {}", fd, buf_ptr, count);
    let buf = instance.memory_offset_addr(0, buf_ptr as usize) as *const c_void;
    unsafe { write(fd, buf, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    let pathname = vararg!(pathname, u32, instance, varargs);
    let flags = vararg!(flags, u32, instance, varargs);
    let mode = vararg!(mode, u32, instance, varargs);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    -2
}

// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    let stream = vararg!(stream, u32, instance, varargs);
    let op = vararg!(op, u32, instance, varargs);
    debug!("stream: {}, op: {}", stream, op);
    0
}

// sys_newuname
pub extern "C" fn ___syscall122(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    let buf = vararg!(buf, u32, instance, varargs);
    debug!("buf: {}", buf);
    0
}
