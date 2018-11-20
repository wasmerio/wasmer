/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset

use libc::{
    c_int,
    c_void,
    size_t,
    ssize_t,
    exit,
    read,
    open,
    close,
};

use crate::webassembly::{Instance};

/// emscripten: ___syscall1
pub extern "C" fn sys_exit(status: c_int) {
    unsafe { exit(status); }
}

/// emscripten: ___syscall3
pub extern "C" fn sys_read(fd: c_int, buf: u32, count: size_t, instance: &mut Instance) -> ssize_t {
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
    unsafe { read(fd, buf_addr, count) }
}

/// emscripten: ___syscall5
pub extern "C" fn sys_open(path: u32, flags: c_int, mode: c_int, instance: &mut Instance) -> c_int {
    let path_addr = instance.memory_offset_addr(0, path as usize) as *const i8;
    unsafe { open(path_addr, flags, mode) }
}

/// emscripten: ___syscall6
pub extern "C" fn sys_close(fd: c_int) -> c_int {
    unsafe { close(fd) }
}

