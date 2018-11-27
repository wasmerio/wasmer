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
    msghdr, getpid, getppid, pid_t,
    gid_t, getgid, fstat, stat,
    pread, mkdir, chown,
};
use crate::webassembly::Instance;
use super::varargs::VarArgs;
use std::{slice, mem};
use super::utils::copy_stat_into_wasm;

/// sys_exit
pub extern "C" fn ___syscall1(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) {
    debug!("emscripten::___syscall1");
    let status: i32 = varargs.get(instance);
    unsafe { exit(status); }
}

/// sys_read
pub extern "C" fn ___syscall3(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall3");
    let fd: i32 = varargs.get(instance);
    let buf: u32 = varargs.get(instance);
    let count: usize = varargs.get(instance);
    debug!("fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
    unsafe { read(fd, buf_addr, count) }
}

/// sys_write
pub extern "C" fn ___syscall4(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall4");
    let fd: i32 = varargs.get(instance);
    let buf: u32 = varargs.get(instance);
    let count: u32 = varargs.get(instance);
    debug!("fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *const c_void;
    unsafe { write(fd, buf_addr, count as usize) as i32 }
}

/// sys_open
pub extern "C" fn ___syscall5(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall5");
    let pathname: u32 = varargs.get(instance);
    let flags: i32 = varargs.get(instance);
    let mode: u32 = varargs.get(instance);
    debug!("pathname: {}, flags: {}, mode: {}", pathname, flags, mode);
    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;
    unsafe {
        let s = std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap();
        debug!("path: {}", s);
    }
    let fd = unsafe { open(pathname_addr, flags, mode) };
    debug!("fd: {}", fd);
    fd
}

/// sys_close
pub extern "C" fn ___syscall6(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall6");
    let fd: i32 = varargs.get(instance);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

/// sys_ioctl
pub extern "C" fn ___syscall54(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall54");
    // let fd: i32 = varargs.get(instance);
    // let request: u32 = varargs.get(instance);
    // debug!("fd: {}, op: {}", fd, request);
    // unsafe { ioctl(fd, request as _) }
    0
}

/// sys_uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub extern "C" fn ___syscall122(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall122");
    let buf: u32 = varargs.get(instance);
    debug!("buf: {}", buf);
    let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut utsname;
    unsafe { uname(buf_addr) }
}

/// sys_lseek
pub extern "C" fn ___syscall140(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> off_t {
    debug!("emscripten::___syscall145");
    let fd: i32 = varargs.get(instance);
    let offset: i64 = varargs.get(instance);
    let whence: i32 = varargs.get(instance);
    debug!("fd: {}, offset: {}, whence = {}", fd, offset, whence);
    unsafe { lseek(fd, offset, whence) }
}

/// sys_readv
pub extern "C" fn ___syscall145(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall145");
    let fd: i32 = varargs.get(instance);
    let iov: u32 = varargs.get(instance);
    let iovcnt: i32 = varargs.get(instance);
    debug!("fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let iov_addr = instance.memory_offset_addr(0, iov as usize) as *mut iovec;
    unsafe { readv(fd, iov_addr, iovcnt) }
}

// sys_writev
pub extern "C" fn ___syscall146(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> ssize_t {
    debug!("emscripten::___syscall145");
    let fd: i32 = varargs.get(instance);
    let iov: u32 = varargs.get(instance);
    let iovcnt: i32 = varargs.get(instance);
    debug!("fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let iov_addr = instance.memory_offset_addr(0, iov as usize) as *mut iovec;
    unsafe { writev(fd, iov_addr, iovcnt) }
}

// /// sys_fcntl64
// pub extern "C" fn ___syscall221(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
//     debug!("emscripten::___syscall221");
//     let fd: i32 = varargs.get(instance);
//     let cmd: i32 = varargs.get(instance);
//     debug!("fd: {}, cmd: {}", fd, cmd);
//     unsafe { fcntl(fd, cmd) }
// }

// sys_socketcall
pub extern "C" fn ___syscall102(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall102");
    let call: u32 = varargs.get(instance);
    let mut socket_varargs: VarArgs = varargs.get(instance);
    match call {
        1 => { // socket (domain: c_int, ty: c_int, protocol: c_int) -> c_int
            let domain: i32 = socket_varargs.get(instance);
            let ty: i32 = socket_varargs.get(instance);
            let protocol: i32 = socket_varargs.get(instance);
            let socket = unsafe { socket(domain, ty, protocol) };
            debug!("socket: {}", socket);
            socket
        },
        2 => { // bind (socket: c_int, address: *const sockaddr, address_len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { bind(socket, address, address_len) }
        },
        3 => { // connect (socket: c_int, address: *const sockaddr, len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { connect(socket, address, address_len) }
        },
        4 => { // listen (socket: c_int, backlog: c_int) -> c_int
            let socket: i32 = socket_varargs.get(instance);
            let backlog: i32 = socket_varargs.get(instance);
            unsafe { listen(socket, backlog) }
        },
        5 => { // accept (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { accept(socket, address, address_len_addr) }
        },
        6 => { // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { getsockname(socket, address, address_len_addr) }
        },
        7 => { // getpeername (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { getpeername(socket, address, address_len_addr) }
        },
        11 => { // sendto (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket: i32 = socket_varargs.get(instance);
            let buf: u32 = socket_varargs.get(instance);
            let flags: usize = socket_varargs.get(instance);
            let len: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            unsafe { sendto(socket, buf_addr, flags, len, address, address_len) as i32 }
        },
        12 => { // recvfrom (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket: i32 = socket_varargs.get(instance);
            let buf: u32 = socket_varargs.get(instance);
            let flags: usize = socket_varargs.get(instance);
            let len: i32 = socket_varargs.get(instance);
            let address: u32 = socket_varargs.get(instance);
            let address_len: u32 = socket_varargs.get(instance);
            let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
            let address = instance.memory_offset_addr(0, address as usize) as *mut sockaddr;
            let address_len_addr = instance.memory_offset_addr(0, address_len as usize) as *mut socklen_t;
            unsafe { recvfrom(socket, buf_addr, flags, len, address, address_len_addr) as i32 }
        },
        14 => { // setsockopt (socket: c_int, level: c_int, name: c_int, value: *const c_void, option_len: socklen_t) -> c_int
            // let socket: i32 = socket_varargs.get(instance);
            // debug!("socket: {}", socket);
            // let level: i32 = socket_varargs.get(instance);
            // debug!("level: {}", level);
            // let name: i32 = socket_varargs.get(instance);
            // debug!("name: {}", name);
            // let value: u32 = socket_varargs.get(instance);
            // debug!("value: {:#x}", value);
            // let option_len: u32 = socket_varargs.get(instance);
            // debug!("option_len: {}", option_len);
            // let value_addr = instance.memory_offset_addr(0, value as usize) as *const c_void;
            
            // let ret = unsafe { setsockopt(socket, level, name, value_addr, option_len) };
            // debug!("ret: {}", ret);
            // if ret != 0 {
            //     debug!("errno: {}", nix::errno::errno());
            // }
            // ret
            0
        },
        15 => { // getsockopt (sockfd: c_int, level: c_int, optname: c_int, optval: *mut c_void, optlen: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(instance);
            let level: i32 = socket_varargs.get(instance);
            let name: i32 = socket_varargs.get(instance);
            let value: u32 = socket_varargs.get(instance);
            let option_len: u32 = socket_varargs.get(instance);
            let value_addr = instance.memory_offset_addr(0, value as usize) as *mut c_void;
            let option_len_addr = instance.memory_offset_addr(0, option_len as usize) as *mut socklen_t;
            unsafe { getsockopt(socket, level, name, value_addr, option_len_addr) }
        },
        16 => { // sendmsg (fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(instance);
            let msg: u32 = socket_varargs.get(instance);
            let flags: i32 = socket_varargs.get(instance);
            let msg_addr = instance.memory_offset_addr(0, msg as usize) as *const msghdr;
            unsafe { sendmsg(socket, msg_addr, flags) as i32 }
        },
        17 => { // recvmsg (fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(instance);
            let msg: u32 = socket_varargs.get(instance);
            let flags: i32 = socket_varargs.get(instance);
            let msg_addr = instance.memory_offset_addr(0, msg as usize) as *mut msghdr;
            unsafe { recvmsg(socket, msg_addr, flags) as i32 }
        },
        _ => { // others
            -1
        },
    }
}

// sys_getpid
pub extern "C" fn ___syscall20() -> pid_t {
    debug!("emscripten::___syscall20");
    unsafe {
        getpid()
    }
}

// sys_getppid
pub extern "C" fn ___syscall64() -> pid_t {
    debug!("emscripten::___syscall64");
    unsafe {
        getpid()
    }
}


// sys_getgid
pub extern "C" fn ___syscall201() -> gid_t {
    debug!("emscripten::___syscall201");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid()
    }
}

// sys_getgid32
pub extern "C" fn ___syscall202() -> gid_t {
    debug!("emscripten::___syscall202");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid()
    }
}

// sys_prlimit64
pub extern "C" fn ___syscall340(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall340");
    // NOTE: Doesn't really matter. Wasm modules cannot exceed WASM_PAGE_SIZE anyway.
    0
}


// sys_fstat64
pub extern "C" fn ___syscall197(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall197");
    let fd: c_int = varargs.get(instance);
    let buf: u32 = varargs.get(instance);

    unsafe {
        let mut stat = std::mem::zeroed();
        let ret = fstat(fd, &mut stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(instance, buf, &stat);
    }

    0
}

// sys_pread
pub extern "C" fn ___syscall180(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall180");
    let fd: i32 = varargs.get(instance);
    let buf: u32 = varargs.get(instance);
    let count: u32 = varargs.get(instance);
    {
        let zero: u32 = varargs.get(instance);
        assert_eq!(zero, 0);
    }
    let offset: i64 = varargs.get(instance);

    let buf_ptr = instance.memory_offset_addr(0, buf as _) as _;

    unsafe {
        pread(fd, buf_ptr, count as _, offset) as _
    }
}

// sys_mkdir
pub extern "C" fn ___syscall39(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall39");
    let pathname: u32 = varargs.get(instance);
    let mode: u32 = varargs.get(instance);
    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;
    
    unsafe {
        mkdir(pathname_addr, mode as _)
    }
}

// sys_stat64
pub extern "C" fn ___syscall195(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall195");
    let pathname: u32 = varargs.get(instance);
    let buf: u32 = varargs.get(instance);

    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;

    unsafe {
        let mut _stat: stat = std::mem::zeroed();
        let ret = stat(pathname_addr, &mut _stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(instance, buf, &_stat);
    }
    0
}

// sys_chown
pub extern "C" fn ___syscall212(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    debug!("emscripten::___syscall212");

    let pathname: u32 = varargs.get(instance);
    let owner: u32 = varargs.get(instance);
    let group: u32 = varargs.get(instance);

    let pathname_addr = instance.memory_offset_addr(0, pathname as usize) as *const i8;

    unsafe {
        chown(pathname_addr, owner, group)
    }
}

// sys_fcntl64
pub extern "C" fn ___syscall221(_which: c_int, mut varargs: VarArgs, instance: &mut Instance) -> c_int {
    let fd: i32 = varargs.get(instance);
    let cmd: u32 = varargs.get(instance);
    match cmd {
        2 => 0,
        _ => -1,
    }
}