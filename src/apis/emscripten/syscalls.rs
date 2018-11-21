/// NOTE: These syscalls only support wasm_32 for now because they take u32 offset

use libc::{
    c_int,
    c_void,
    size_t,
    ssize_t,
    exit,
    read,
    write,
    open,
    close,
};
use std::os::raw::c_char;
use std::ffi::CStr;

use crate::webassembly::{Instance};

/// emscripten: ___syscall3 (sys_read)
pub extern "C" fn ___syscall3(which: c_int, varargs: c_int, instance: &mut Instance) -> ssize_t {
    // function ___syscall3(which, varargs) {
    //   which, varargs
    //   SYSCALLS.varargs = varargs;
    //   try {
    //     var stream = SYSCALLS.getStreamFromFD(),
    //       buf = SYSCALLS.get(),
    //       count = SYSCALLS.get();
    //     return FS.read(stream, HEAP8, buf, count);
    //   } catch (e) {
    //     if (typeof FS === "undefined" || !(e instanceof FS.ErrnoError)) abort(e);
    //     return -e.errno;
    //   }
    // }
    debug!("emscripten::___syscall3({}, {})", which, varargs);
    0
}

/// emscripten: ___syscall4 (sys_write)
pub extern "C" fn ___syscall4(which_ptr: c_int, varargs_ptr: c_int, instance: &mut Instance) -> c_int {
    // function ___syscall4(which, varargs) {
    //   SYSCALLS.varargs = varargs;
    //   try {
    //     var stream = SYSCALLS.getStreamFromFD(),
    //       buf = SYSCALLS.get(),
    //       count = SYSCALLS.get();
    //     return FS.write(stream, HEAP8, buf, count);
    //   } catch (e) {
    //     if (typeof FS === "undefined" || !(e instanceof FS.ErrnoError)) abort(e);
    //     return -e.errno;
    //   }
    // }
    debug!("emscripten::___syscall4({}, {})", which_ptr, varargs_ptr);
    0
}
/// emscripten: ___syscall5 (sys_open)
pub extern "C" fn ___syscall5(which: c_int, varargs: c_int, instance: &mut Instance) -> c_int {
    // function ___syscall5(which, varargs) {
    //   SYSCALLS.varargs = varargs;
    //   try {
    //     var pathname = SYSCALLS.getStr(),
    //       flags = SYSCALLS.get(),
    //       mode = SYSCALLS.get();
    //     var stream = FS.open(pathname, flags, mode);
    //     return stream.fd;
    //   } catch (e) {
    //     if (typeof FS === "undefined" || !(e instanceof FS.ErrnoError)) abort(e);
    //     return -e.errno;
    //   }
    // }
    debug!("host::___syscall5({}, {})", which, varargs);
    -2
}
