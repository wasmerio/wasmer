/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{c_int, c_void, ssize_t, write};

use crate::webassembly::Instance;
use super::varargs::VarArgs;

/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, mut _varargs: VarArgs, _instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    0
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    let fd: i32 = varargs.get(instance);
    let buf_ptr: u32 = varargs.get(instance);
    let count: u32 = varargs.get(instance);
    debug!("fd: {}, buf_ptr: {}, count: {}", fd, buf_ptr, count);
    let buf = instance.memory_offset_addr(0, buf_ptr as usize) as *const c_void;
    unsafe { write(fd, buf, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    let pathname: u32 = varargs.get(instance);
    let flags: u32 = varargs.get(instance);
    let mode: u32 = varargs.get(instance);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    -2
}

// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    let stream: u32 = varargs.get(instance);
    let op: u32 = varargs.get(instance);
    debug!("stream: {}, op: {}", stream, op);
    0
}

// sys_newuname
pub extern "C" fn ___syscall122(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    let buf: u32 = varargs.get(instance);
    debug!("buf: {}", buf);
    0
}
