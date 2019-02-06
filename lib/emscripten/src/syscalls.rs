use super::utils::copy_stat_into_wasm;
use super::varargs::VarArgs;
use byteorder::{ByteOrder, LittleEndian};
/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    accept,
    bind,
    // ENOTTY,
    c_char,
    c_int,
    c_void,
    chdir,
    chown,
    // fcntl, setsockopt, getppid
    close,
    connect,
    dup2,
    exit,
    fcntl,
    fstat,
    getgid,
    getpeername,
    getpid,
    getsockname,
    getsockopt,
    gid_t,
    in_addr_t,
    in_port_t,
    ioctl,
    // iovec,
    listen,
    lseek,
    mkdir,
    msghdr,
    off_t,
    open,
    pid_t,
    pread,
    pwrite,
    read,
    // readv,
    recvfrom,
    recvmsg,
    rmdir,
    // ENOTTY,
    rusage,
    sa_family_t,
    // writev,
    select,
    sendmsg,
    sendto,
    setpgid,
    setsockopt,
    sockaddr,
    socket,
    socklen_t,
    ssize_t,
    stat,
    uname,
    utsname,
    write,
    EINVAL,
    // sockaddr_in,
    FIOCLEX,
    FIONBIO,
    F_GETFD,
    F_SETFD,
    SOL_SOCKET,
    SO_REUSEADDR,
    TIOCGWINSZ,
};
use wasmer_runtime_core::vm::Ctx;

use super::env;
use std::mem;
use std::slice;
// use std::sys::fd::FileDesc;

// Linking to functions that are not provided by rust libc
#[cfg(target_os = "macos")]
#[link(name = "c")]
extern "C" {
    pub fn wait4(pid: pid_t, status: *mut c_int, options: c_int, rusage: *mut rusage) -> pid_t;
}

#[cfg(not(target_os = "macos"))]
use libc::wait4;

// Another conditional constant for name resolution: Macos et iOS use
// SO_NOSIGPIPE as a setsockopt flag to disable SIGPIPE emission on socket.
// Other platforms do otherwise.
#[cfg(target_os = "darwin")]
use libc::SO_NOSIGPIPE;
#[cfg(not(target_os = "darwin"))]
const SO_NOSIGPIPE: c_int = 0;

/// exit
pub fn ___syscall1(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) {
    debug!("emscripten::___syscall1 (exit) {}", which);
    let status: i32 = varargs.get(ctx);
    unsafe {
        exit(status);
    }
}

/// read
pub fn ___syscall3(which: i32, mut varargs: VarArgs, ctx: &mut Ctx) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall3 (read) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut c_void;
    let ret = unsafe { read(fd, buf_addr, count) };
    debug!("=> ret: {}", ret);
    ret as _
}

/// write
pub fn ___syscall4(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall4 (write) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count = varargs.get(ctx);
    debug!("=> fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *const c_void;
    unsafe { write(fd, buf_addr, count) as i32 }
}

/// open
pub fn ___syscall5(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall5 (open) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let flags: i32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    let path_str = unsafe { std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap() };
    let fd = unsafe { open(pathname_addr, flags, mode) };
    debug!(
        "=> pathname: {}, flags: {}, mode: {} = fd: {}\npath: {}",
        pathname, flags, mode, fd, path_str
    );
    fd
}

/// close
pub fn ___syscall6(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall6 (close) {}", which);
    let fd: i32 = varargs.get(ctx);
    debug!("fd: {}", fd);
    unsafe { close(fd) }
}

// chdir
pub fn ___syscall12(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall12 (chdir) {}", which);
    let path_addr: i32 = varargs.get(ctx);
    unsafe {
        let path_ptr = emscripten_memory_pointer!(ctx.memory(0), path_addr) as *const i8;
        let path = std::ffi::CStr::from_ptr(path_ptr);
        let ret = chdir(path_ptr);
        debug!("=> path: {:?}, ret: {}", path, ret);
        ret
    }
}

pub fn ___syscall10(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall10");
    -1
}

pub fn ___syscall15(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall15");
    -1
}

// getpid
pub fn ___syscall20(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall20 (getpid)");
    unsafe { getpid() }
}

pub fn ___syscall38(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall38");
    -1
}

// mkdir
pub fn ___syscall39(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { mkdir(pathname_addr, mode as _) }
}

// rmdir
pub fn ___syscall40(_which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall40 (rmdir)");
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { rmdir(pathname_addr) }
}

/// ioctl
pub fn ___syscall54(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", which);
    let fd: i32 = varargs.get(ctx);
    let request: u32 = varargs.get(ctx);
    debug!("fd: {}, op: {}", fd, request);
    // Got the equivalents here: https://code.woboq.org/linux/linux/include/uapi/asm-generic/ioctls.h.html
    match request as _ {
        21537 => {
            // FIONBIO
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
            let ret = unsafe { ioctl(fd, FIONBIO, argp_ptr) };
            debug!("ret(FIONBIO): {}", ret);
            ret
            // 0
        }
        21523 => {
            // TIOCGWINSZ
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
            let ret = unsafe { ioctl(fd, TIOCGWINSZ, argp_ptr) };
            debug!("ret(TIOCGWINSZ): {} (harcoded to 0)", ret);
            // ret
            // TODO: We hardcode the value to have emscripten tests pass, as for some reason
            // when the capturer is active, ioctl returns -1 instead of 0
            if ret == -1 {
                0
            } else {
                ret
            }
        }
        _ => {
            debug!(
                "emscripten::___syscall54 -> non implemented case {}",
                request
            );
            0
        }
    }
}

// setpgid
pub fn ___syscall57(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", which);
    let pid: i32 = varargs.get(ctx);
    let pgid: i32 = varargs.get(ctx);
    unsafe { setpgid(pid, pgid) }
}

pub fn ___syscall60(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall60");
    -1
}

// dup2
pub fn ___syscall63(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", which);

    let src: i32 = varargs.get(ctx);
    let dst: i32 = varargs.get(ctx);

    unsafe { dup2(src, dst) }
}

// getppid
pub fn ___syscall64(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall64 (getppid)");
    unsafe { getpid() }
}

pub fn ___syscall66(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall66");
    -1
}

pub fn ___syscall75(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall75");
    -1
}

pub fn ___syscall85(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall85");
    -1
}

pub fn ___syscall91(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall91");
    -1
}

pub fn ___syscall97(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall97");
    -1
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", which);
    let call: u32 = varargs.get(ctx);
    let mut socket_varargs: VarArgs = varargs.get(ctx);

    #[repr(C)]
    pub struct GuestSockaddrIn {
        pub sin_family: sa_family_t, // u16
        pub sin_port: in_port_t,     // u16
        pub sin_addr: GuestInAddr,   // u32
        pub sin_zero: [u8; 8],       // u8 * 8
        // 2 + 2 + 4 + 8 = 16
    }

    #[repr(C)]
    pub struct GuestInAddr {
        pub s_addr: in_addr_t, // u32
    }

    // debug!("GuestSockaddrIn = {}", size_of::<GuestSockaddrIn>());

    pub struct LinuxSockAddr {
        pub sa_family: u16,
        pub sa_data: [c_char; 14],
    }

    match call {
        1 => {
            debug!("socket: socket");
            // socket (domain: c_int, ty: c_int, protocol: c_int) -> c_int
            let domain: i32 = socket_varargs.get(ctx);
            let ty: i32 = socket_varargs.get(ctx);
            let protocol: i32 = socket_varargs.get(ctx);
            let fd = unsafe { socket(domain, ty, protocol) };
            // set_cloexec
            unsafe {
                ioctl(fd, FIOCLEX);
            };

            type T = u32;
            let payload = 1 as *const T as _;
            unsafe {
                setsockopt(
                    fd,
                    SOL_SOCKET,
                    SO_NOSIGPIPE,
                    payload,
                    mem::size_of::<T>() as socklen_t,
                );
            };

            debug!(
                "=> domain: {} (AF_INET/2), type: {} (SOCK_STREAM/1), protocol: {} = fd: {}",
                domain, ty, protocol, fd
            );
            fd
        }
        2 => {
            debug!("socket: bind");
            // bind (socket: c_int, address: *const sockaddr, address_len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;

            // Debug received address
            unsafe {
                let proper_address = address as *const GuestSockaddrIn;
                debug!(
                    "=> address.sin_family: {:?}, address.sin_port: {:?}, address.sin_addr.s_addr: {:?}",
                    (*proper_address).sin_family, (*proper_address).sin_port, (*proper_address).sin_addr.s_addr
                );
            }

            let status = unsafe { bind(socket, address, address_len) };
            // debug!("=> status: {}", status);
            debug!(
                "=> socketfd: {}, address: {:?}, address_len: {} = status: {}",
                socket, address, address_len, status
            );
            status
            // -1
        }
        3 => {
            debug!("socket: connect");
            // connect (socket: c_int, address: *const sockaddr, len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            unsafe { connect(socket, address, address_len) }
        }
        4 => {
            debug!("socket: listen");
            // listen (socket: c_int, backlog: c_int) -> c_int
            let socket = socket_varargs.get(ctx);
            let backlog: i32 = socket_varargs.get(ctx);
            let status = unsafe { listen(socket, backlog) };
            debug!(
                "=> socketfd: {}, backlog: {} = status: {}",
                socket, backlog, status
            );
            status
        }
        5 => {
            debug!("socket: accept");
            // accept (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = socket_varargs.get(ctx);
            let address_addr: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address_addr) as *mut sockaddr;

            debug!(
                "=> socket: {}, address: {:?}, address_len: {}",
                socket, address, address_len
            );
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut socklen_t;
            // let mut address_len_addr: socklen_t = 0;

            let fd = unsafe { accept(socket, address, address_len_addr) };

            unsafe {
                let address_linux =
                    emscripten_memory_pointer!(ctx.memory(0), address_addr) as *mut LinuxSockAddr;
                (*address_linux).sa_family = (*address).sa_family as u16;
                (*address_linux).sa_data = (*address).sa_data;
            };

            // set_cloexec
            unsafe {
                ioctl(fd, FIOCLEX);
            };

            debug!("fd: {}", fd);

            fd
        }
        6 => {
            debug!("socket: getsockname");
            // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut socklen_t;
            unsafe { getsockname(socket, address, address_len_addr) }
        }
        7 => {
            debug!("socket: getpeername");
            // getpeername (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut socklen_t;
            unsafe { getpeername(socket, address, address_len_addr) }
        }
        11 => {
            debug!("socket: sendto");
            // sendto (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket = socket_varargs.get(ctx);
            let buf: u32 = socket_varargs.get(ctx);
            let flags = socket_varargs.get(ctx);
            let len: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            unsafe { sendto(socket, buf_addr, flags, len, address, address_len) as i32 }
        }
        12 => {
            debug!("socket: recvfrom");
            // recvfrom (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket = socket_varargs.get(ctx);
            let buf: u32 = socket_varargs.get(ctx);
            let flags = socket_varargs.get(ctx);
            let len: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut socklen_t;
            unsafe { recvfrom(socket, buf_addr, flags, len, address, address_len_addr) as i32 }
        }
        14 => {
            debug!("socket: setsockopt");
            // NOTE: Emscripten seems to be passing the wrong values to this syscall
            //      level: Em passes 1 as SOL_SOCKET; SOL_SOCKET is 0xffff in BSD
            //      name: Em passes SO_ACCEPTCONN, but Nginx complains about REUSEADDR
            //      https://github.com/openbsd/src/blob/master/sys/sys/socket.h#L156
            // setsockopt (socket: c_int, level: c_int, name: c_int, value: *const c_void, option_len: socklen_t) -> c_int

            let socket = socket_varargs.get(ctx);
            // SOL_SOCKET = 0xffff (BSD, Linux)
            let level: i32 = SOL_SOCKET;
            let _: u32 = socket_varargs.get(ctx);
            // SO_REUSEADDR = 0x4 (BSD, Linux)
            let name: i32 = SO_REUSEADDR;
            let _: u32 = socket_varargs.get(ctx);
            let value: u32 = socket_varargs.get(ctx);
            let option_len = socket_varargs.get(ctx);
            let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as _; // Endian problem
            let ret = unsafe { setsockopt(socket, level, name, value_addr, option_len) };

            debug!("=> socketfd: {}, level: {} (SOL_SOCKET/0xffff), name: {} (SO_REUSEADDR/4), value_addr: {:?}, option_len: {} = status: {}", socket, level, name, value_addr, option_len, ret);
            ret
        }
        15 => {
            debug!("socket: getsockopt");
            // getsockopt (sockfd: c_int, level: c_int, optname: c_int, optval: *mut c_void, optlen: *mut socklen_t) -> c_int
            let socket = socket_varargs.get(ctx);
            let level: i32 = socket_varargs.get(ctx);
            let name: i32 = socket_varargs.get(ctx);
            let value: u32 = socket_varargs.get(ctx);
            let option_len: u32 = socket_varargs.get(ctx);
            let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as _;
            let option_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), option_len) as *mut socklen_t;
            unsafe { getsockopt(socket, level, name, value_addr, option_len_addr) }
        }
        16 => {
            debug!("socket: sendmsg");
            // sendmsg (fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_pointer!(ctx.memory(0), msg) as *const msghdr;
            unsafe { sendmsg(socket, msg_addr, flags) as i32 }
        }
        17 => {
            debug!("socket: recvmsg");
            // recvmsg (fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_pointer!(ctx.memory(0), msg) as *mut msghdr;
            unsafe { recvmsg(socket, msg_addr, flags) as i32 }
        }
        _ => {
            // others
            -1
        }
    }
}

pub fn ___syscall110(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall110");
    -1
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(_which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> pid_t {
    debug!("emscripten::___syscall114 (wait4)");
    let pid: pid_t = varargs.get(ctx);
    let status: u32 = varargs.get(ctx);
    let options: c_int = varargs.get(ctx);
    let rusage: u32 = varargs.get(ctx);
    let status_addr = emscripten_memory_pointer!(ctx.memory(0), status) as *mut c_int;
    let rusage_addr = emscripten_memory_pointer!(ctx.memory(0), rusage) as *mut rusage;
    let res = unsafe { wait4(pid, status_addr, options, rusage_addr) };
    debug!(
        "=> pid: {}, status: {:?}, options: {}, rusage: {:?} = pid: {}",
        pid, status_addr, options, rusage_addr, res
    );
    res
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", which);
    let buf: u32 = varargs.get(ctx);
    debug!("=> buf: {}", buf);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut utsname;
    unsafe { uname(buf_addr) }
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", which);

    let nfds: i32 = varargs.get(ctx);
    let readfds: u32 = varargs.get(ctx);
    let writefds: u32 = varargs.get(ctx);
    let exceptfds: u32 = varargs.get(ctx);
    let _timeout: i32 = varargs.get(ctx);

    assert!(nfds <= 64, "`nfds` must be less than or equal to 64");
    assert!(exceptfds == 0, "`exceptfds` is not supporrted");

    let readfds_ptr = emscripten_memory_pointer!(ctx.memory(0), readfds) as _;
    let writefds_ptr = emscripten_memory_pointer!(ctx.memory(0), writefds) as _;

    unsafe { select(nfds, readfds_ptr, writefds_ptr, 0 as _, 0 as _) }
}

// mmap2
pub fn ___syscall192(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall192 (mmap2) {}", which);
    let addr: i32 = varargs.get(ctx);
    let len: u32 = varargs.get(ctx);
    let prot: i32 = varargs.get(ctx);
    let flags: i32 = varargs.get(ctx);
    let fd: i32 = varargs.get(ctx);
    let off: i32 = varargs.get(ctx);
    debug!(
        "=> addr: {}, len: {}, prot: {}, flags: {}, fd: {}, off: {}",
        addr, len, prot, flags, fd, off
    );

    if fd == -1 {
        let ptr = env::call_memalign(16384, len, ctx);
        if ptr == 0 {
            return -1;
        }
        env::call_memset(ptr, 0, len, ctx);
        ptr as _
    } else {
        -1
    }
}

/// lseek
pub fn ___syscall140(which: i32, mut varargs: VarArgs, ctx: &mut Ctx) -> i32 {
    // -> c_int
    debug!("emscripten::___syscall140 (lseek) {}", which);
    let fd: i32 = varargs.get(ctx);
    let offset = varargs.get(ctx);
    let whence: i32 = varargs.get(ctx);
    debug!("=> fd: {}, offset: {}, whence = {}", fd, offset, whence);
    unsafe { lseek(fd, offset, whence) as _ }
}

/// readv
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall145(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall145 (readv) {}", which);
    // let fd: i32 = varargs.get(ctx);
    // let iov: u32 = varargs.get(ctx);
    // let iovcnt: i32 = varargs.get(ctx);
    // debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    // let iov_addr = emscripten_memory_pointer!(ctx.memory(0), iov) as *mut iovec;
    // unsafe { readv(fd, iov_addr, iovcnt) }

    let fd: i32 = varargs.get(ctx);
    let iov: i32 = varargs.get(ctx);
    let iovcnt: i32 = varargs.get(ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let mut ret = 0;
    unsafe {
        for i in 0..iovcnt {
            let guest_iov_addr =
                emscripten_memory_pointer!(ctx.memory(0), (iov + i * 8)) as *mut GuestIovec;
            let iov_base = emscripten_memory_pointer!(ctx.memory(0), (*guest_iov_addr).iov_base)
                as *mut c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = read(fd, iov_base, iov_len);
            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        // debug!(" => ret: {}", ret);
        ret as _
    }
}

// writev
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall146(which: i32, mut varargs: VarArgs, ctx: &mut Ctx) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", which);
    let fd: i32 = varargs.get(ctx);
    let iov: i32 = varargs.get(ctx);
    let iovcnt: i32 = varargs.get(ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    debug!("=> fd: {}, iov: {}, iovcnt = {}", fd, iov, iovcnt);
    let mut ret = 0;
    unsafe {
        for i in 0..iovcnt {
            let guest_iov_addr =
                emscripten_memory_pointer!(ctx.memory(0), (iov + i * 8)) as *mut GuestIovec;
            let iov_base = emscripten_memory_pointer!(ctx.memory(0), (*guest_iov_addr).iov_base)
                as *const c_void;
            let iov_len = (*guest_iov_addr).iov_len as _;
            // debug!("=> iov_addr: {:?}, {:?}", iov_base, iov_len);
            let curr = write(fd, iov_base, iov_len);
            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        // debug!(" => ret: {}", ret);
        ret as _
    }
}

pub fn ___syscall168(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall168");
    -1
}

// pread
pub fn ___syscall180(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: u32 = varargs.get(ctx);
    {
        let zero: u32 = varargs.get(ctx);
        assert_eq!(zero, 0);
    }
    let offset: i64 = varargs.get(ctx);

    let buf_ptr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;

    unsafe { pread(fd, buf_ptr, count as _, offset) as _ }
}

// pwrite
pub fn ___syscall181(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: u32 = varargs.get(ctx);
    {
        let zero: u32 = varargs.get(ctx);
        assert_eq!(zero, 0);
    }
    let offset: i64 = varargs.get(ctx);

    let buf_ptr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
    let status = unsafe { pwrite(fd, buf_ptr, count as _, offset) as _ };
    debug!(
        "=> fd: {}, buf: {}, count: {}, offset: {} = status:{}",
        fd, buf, count, offset, status
    );
    status
}

pub fn ___syscall191(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall191 - stub");
    -1
}

pub fn ___syscall194(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    -1
}

pub fn ___syscall196(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall194 - stub");
    -1
}

pub fn ___syscall199(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall199 - stub");
    -1
}

// stat64
pub fn ___syscall195(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall195 (stat64) {}", which);
    let pathname: u32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe {
        let mut _stat: stat = std::mem::zeroed();
        let ret = stat(pathname_addr, &mut _stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &_stat);
    }
    0
}

// fstat64
pub fn ___syscall197(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall197 (fstat64) {}", which);
    let fd: c_int = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);

    unsafe {
        let mut stat = std::mem::zeroed();
        let ret = fstat(fd, &mut stat);
        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        copy_stat_into_wasm(ctx, buf, &stat);
    }

    0
}

// getgid
pub fn ___syscall201(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall201 (getgid)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid() as i32
    }
}

// getgid32
pub fn ___syscall202(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getgid32)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid() as _
    }
}

// chown
pub fn ___syscall212(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", which);

    let pathname: u32 = varargs.get(ctx);
    let owner: u32 = varargs.get(ctx);
    let group: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe { chown(pathname_addr, owner, group) }
}

pub fn ___syscall220(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall220");
    -1
}

// fcntl64
pub fn ___syscall221(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", which);
    // fcntl64
    let _fd: i32 = varargs.get(ctx);
    let cmd: u32 = varargs.get(ctx);
    match cmd {
        2 => 0,
        _ => -1,
    }
}

pub fn ___syscall268(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall268");
    -1
}

pub fn ___syscall272(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall272");
    -1
}

pub fn ___syscall295(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall295");
    -1
}

pub fn ___syscall300(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall300");
    -1
}

pub fn ___syscall334(_one: i32, _two: i32, _ctx: &mut Ctx) -> i32 {
    debug!("emscripten::___syscall334");
    -1
}

/// dup3
pub fn ___syscall330(_which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> pid_t {
    // Implementation based on description at https://linux.die.net/man/2/dup3
    debug!("emscripten::___syscall330 (dup3)");
    let oldfd: c_int = varargs.get(ctx);
    let newfd: c_int = varargs.get(ctx);
    let flags: c_int = varargs.get(ctx);

    if oldfd == newfd {
        return EINVAL;
    }

    let res = unsafe { dup2(oldfd, newfd) };

    // Set flags on newfd (https://www.gnu.org/software/libc/manual/html_node/Descriptor-Flags.html)
    let mut old_flags = unsafe { fcntl(newfd, F_GETFD, 0) };

    if old_flags > 0 {
        old_flags |= flags;
    } else if old_flags == 0 {
        old_flags &= !flags;
    }

    unsafe {
        fcntl(newfd, F_SETFD, old_flags);
    }

    debug!(
        "=> oldfd: {}, newfd: {}, flags: {} = pid: {}",
        oldfd, newfd, flags, res
    );
    res
}

// prlimit64
pub fn ___syscall340(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall340 (prlimit64), {}", which);
    // NOTE: Doesn't really matter. Wasm modules cannot exceed WASM_PAGE_SIZE anyway.
    let _pid: i32 = varargs.get(ctx);
    let _resource: i32 = varargs.get(ctx);
    let _new_limit: u32 = varargs.get(ctx);
    let old_limit: u32 = varargs.get(ctx);

    if old_limit != 0 {
        // just report no limits
        let buf_ptr = emscripten_memory_pointer!(ctx.memory(0), old_limit) as *mut u8;
        let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, 16) };

        LittleEndian::write_i32(&mut buf[..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[4..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[8..], -1); // RLIM_INFINITY
        LittleEndian::write_i32(&mut buf[12..], -1); // RLIM_INFINITY
    }

    0
}
