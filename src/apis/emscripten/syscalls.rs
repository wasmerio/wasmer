/// NOTE: TODO: These syscalls only support wasm_32 for now because they take u32 offset
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    c_int, c_void, utsname,
    ssize_t, write, exit, read,
    open, close, ioctl,
    uname,
};
#[macro_use]
use macros;
use crate::webassembly::Instance;

/// sys_exit
pub extern "C" fn ___syscall1(_which: c_int, varargs: c_int, instance: &mut Instance) {
    debug!("emscripten::___syscall1");
    let status = vararg!(i32, instance, varargs);
    unsafe { exit(status); }
}

/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, varargs: c_int, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    let fd = vararg!(i32, instance, varargs);
    let buf = vararg!(u32, instance, varargs);
    let count = vararg!(usize, instance, varargs);
    debug!("fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
    unsafe { read(fd, buf_addr, count) }
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall4");
    let fd = vararg!(i32, instance, varargs);
    let buf = vararg!(u32, instance, varargs);
    let count = vararg!(u32, instance, varargs);
    debug!("fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *const c_void;
    unsafe { write(fd, buf_addr, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    let pathname = vararg!(u32, instance, varargs);
    let flags = vararg!(i32, instance, varargs);
    let mode = vararg!(u32, instance, varargs);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;
    unsafe { open(pathname_addr, flags, mode) }
}

/// sys_close
pub extern "C" fn ___syscall6(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall1");
    let fd = vararg!(i32, instance, varargs);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    let fd = vararg!(i32, instance, varargs);
    let request = vararg!(u64, instance, varargs);
    debug!("fd: {}, op: {}", fd, request);
    unsafe { ioctl(fd, request) }
}

// sys_newuname
pub extern "C" fn ___syscall122(_which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    let buf = vararg!(u32, instance, varargs);
    debug!("buf: {}", buf);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut utsname;
    unsafe { uname(buf_addr) } // TODO: Fix implementation
}
