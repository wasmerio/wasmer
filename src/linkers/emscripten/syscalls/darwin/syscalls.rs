use libc::{
    c_int,
    c_void,
    c_char,
    size_t,
    ssize_t,
    open,
    exit,
    read,
};

/// exit
pub extern "C" fn __syscall1(status: c_int) {
    unsafe { exit(status); }
}

/// read
pub extern "C" fn __syscall3(fd: c_int, buf: *mut c_void, count: size_t) -> ssize_t {
    unsafe { read(fd, buf, count) }
}

/// open
pub extern "C" fn __syscall5(path: *const c_char, oflag: c_int) -> c_int {
    unsafe { open(path, oflag) }
}
