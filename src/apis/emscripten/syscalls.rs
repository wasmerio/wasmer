/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    c_int, c_void, utsname, off_t,
    ssize_t, write, exit, read,
    open, close, ioctl,
    uname, fcntl, lseek, readv,
    iovec, writev, socklen_t,
    sockaddr, socket, bind,
    connect, listen, accept,
    getsockname, getpeername,
    sendto, recvfrom, setsockopt,
    getsockopt, sendmsg, recvmsg,
    msghdr,
};

use macros;
use crate::webassembly::Instance;

/// sys_exit
pub extern "C" fn ___syscall1(_which: c_int, mut varargs: c_int, instance: &mut Instance) {
    debug!("emscripten::___syscall1");
    let status = vararg!(i32, instance, varargs);
    unsafe { exit(status); }
}

/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    let fd = vararg!(i32, instance, varargs);
    let buf = vararg!(u32, instance, varargs);
    let count = vararg!(usize, instance, varargs);
    debug!("fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
    unsafe { read(fd, buf_addr, count) }
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall4");
    let fd = vararg!(i32, instance, varargs);
    let buf = vararg!(u32, instance, varargs);
    let count = vararg!(u32, instance, varargs);
    debug!("fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *const c_void;
    unsafe { write(fd, buf_addr, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    let pathname = vararg!(u32, instance, varargs);
    let flags = vararg!(i32, instance, varargs);
    let mode = vararg!(u32, instance, varargs);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;
    unsafe { open(pathname_addr, flags, mode) }
}

/// sys_close
pub extern "C" fn ___syscall6(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall1");
    let fd = vararg!(i32, instance, varargs);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

/// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    let fd = vararg!(i32, instance, varargs);
    let request = vararg!(u64, instance, varargs);
    debug!("fd: {}, op: {}", fd, request);
    unsafe { ioctl(fd, request) }
}

/// sys_uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub extern "C" fn ___syscall122(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    let buf = vararg!(u32, instance, varargs);
    debug!("buf: {}", buf);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut utsname;
    unsafe { uname(buf_addr) }
}

/// sys_lseek
pub extern "C" fn ___syscall140(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> off_t {
    debug!("emscripten::___syscall145");
    let fd = vararg!(i32, instance, varargs);
    let offset = vararg!(i64, instance, varargs);
    let whence = vararg!(i32, instance, varargs);
    debug!("fd: {}, offset: {}, whence = {}", fd, offset, whence);
    unsafe { lseek(fd, offset, whence) }
}

/// sys_readv
pub extern "C" fn ___syscall145(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall145");
    let fd = vararg!(i32, instance, varargs);
    let iov = vararg!(u32, instance, varargs); // TODO: struct iovec { iov_base: *mut c_void, iov_len: size_t }
    let iovcnt = vararg!(i32, instance, varargs);
    debug!("fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let iov_addr = instance.memory_offset_addr(0, iov as usize) as *mut iovec;
    unsafe { readv(fd, iov_addr, iovcnt) }
}

// sys_writev
pub extern "C" fn ___syscall146(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall145");
    let fd = vararg!(i32, instance, varargs);
    let iov = vararg!(u32, instance, varargs); // TODO: struct iovec { iov_base: *mut c_void, iov_len: size_t }
    let iovcnt = vararg!(i32, instance, varargs);
    debug!("fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let iov_addr = instance.memory_offset_addr(0, iov as usize) as *mut iovec;
    unsafe { writev(fd, iov_addr, iovcnt) }
}

/// sys_fcntl64
pub extern "C" fn ___syscall221(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall221");
    let fd = vararg!(i32, instance, varargs);
    let cmd = vararg!(i32, instance, varargs);
    debug!("fd: {}, cmd: {}", fd, cmd);
    unsafe { fcntl(fd, cmd) }
}

// sys_socketcall
pub extern "C" fn ___syscall102(_which: c_int, mut varargs: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall102");
    let call = vararg!(u32, instance, varargs);
    match call {
        1 => { // socket (domain: c_int, ty: c_int, protocol: c_int) -> c_int
            let domain = vararg!(i32, instance, varargs);
            let ty = vararg!(i32, instance, varargs);
            let protocol = vararg!(i32, instance, varargs); // NOTE: Emscripten asserts protocol to be TCP (i.e 0x6)
            unsafe { socket(domain, ty, protocol) }
        },
        2 => { // bind (socket: c_int, address: *const sockaddr, address_len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs);
            let address_len = vararg!(u32, instance, varargs);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { bind(socket, address, address_len) }
        },
        3 => { // connect (socket: c_int, address: *const sockaddr, len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs);
            let address_len = vararg!(u32, instance, varargs);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { connect(socket, address, address_len) }
        },
        4 => { // listen (socket: c_int, backlog: c_int) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let backlog = vararg!(i32, instance, varargs);
            unsafe { listen(socket, backlog) }
        },
        5 => { // accept (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs); // TODO: sockaddr has ptr
            let address_len = vararg!(u32, instance, varargs);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { accept(socket, address, address_len_addr) }
        },
        6 => { // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs); // TODO: sockaddr has ptr
            let address_len = vararg!(u32, instance, varargs);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { getsockname(socket, address, address_len_addr) }
        },
        7 => { // getpeername (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs); // TODO: sockaddr has ptr
            let address_len = vararg!(u32, instance, varargs);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { getpeername(socket, address, address_len_addr) }
        },
        11 => { // sendto (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket = vararg!(i32, instance, varargs);
            let buf = vararg!(u32, instance, varargs);
            let flags = vararg!(usize, instance, varargs);
            let len = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs); // TODO: sockaddr has ptr
            let address_len = vararg!(u32, instance, varargs);
            let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { sendto(socket, buf_addr, flags, len, address, address_len) as i32 }
        },
        12 => { // recvfrom (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket = vararg!(i32, instance, varargs);
            let buf = vararg!(u32, instance, varargs);
            let flags = vararg!(usize, instance, varargs);
            let len = vararg!(i32, instance, varargs);
            let address = vararg!(u32, instance, varargs); // TODO: sockaddr has ptr
            let address_len = vararg!(u32, instance, varargs);
            let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { recvfrom(socket, buf_addr, flags, len, address, address_len_addr) as i32 }
        },
        14 => { // setsockopt (socket: c_int, level: c_int, name: c_int, value: *const c_void, option_len: socklen_t) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let level = vararg!(i32, instance, varargs);
            let name = vararg!(i32, instance, varargs);
            let value = vararg!(u32, instance, varargs);
            let option_len = vararg!(u32, instance, varargs);
            let value_addr = instance.memory_offset_addr(0, value as usize) as *const c_void;
            unsafe { setsockopt(socket, level, name, value_addr, option_len) }

        },
        15 => { // getsockopt (sockfd: c_int, level: c_int, optname: c_int, optval: *mut c_void, optlen: *mut socklen_t) -> c_int
            let socket = vararg!(i32, instance, varargs);
            let level = vararg!(i32, instance, varargs);
            let name = vararg!(i32, instance, varargs);
            let value = vararg!(u32, instance, varargs);
            let option_len = vararg!(u32, instance, varargs);
            let value_addr = instance.memory_offset_addr(0, value as usize) as *mut c_void;
            let option_len_addr = instance.memory_offset_addr(0, option_len as usize) as *mut socklen_t;
            unsafe { getsockopt(socket, level, name, value_addr, option_len_addr) }
        },
        16 => { // sendmsg (fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t
            let socket = vararg!(i32, instance, varargs);
            let msg = vararg!(u32, instance, varargs);  // TODO: msghdr has ptr
            let flags = vararg!(i32, instance, varargs);
            let msg_addr = instance.memory_offset_addr(0, msg as usize) as *const msghdr;
            unsafe { sendmsg(socket, msg_addr, flags) as i32 }
        },
        17 => { // recvmsg (fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t
            let socket = vararg!(i32, instance, varargs);
            let msg = vararg!(u32, instance, varargs);  // TODO: msghdr has ptr
            let flags = vararg!(i32, instance, varargs);
            let msg_addr = instance.memory_offset_addr(0, msg as usize) as *mut msghdr;
            unsafe { recvmsg(socket, msg_addr, flags) as i32 }
        },
        _ => { // others
            -1
        },
    }
}
