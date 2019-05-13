use crate::varargs::VarArgs;
#[cfg(target_os = "macos")]
use libc::size_t;
/// NOTE: TODO: These syscalls only support wasm_32 for now because they assume offsets are u32
/// Syscall list: https://www.cs.utexas.edu/~bismith/test/syscalls/syscalls32.html
use libc::{
    accept,
    access,
    bind,
    c_char,
    c_int,
    c_void,
    chown,
    // fcntl, setsockopt, getppid
    connect,
    dup,
    dup2,
    fchmod,
    fchown,
    fcntl,
    // ENOTTY,
    fsync,
    getgid,
    getgroups,
    getpeername,
    getrusage,
    getsockname,
    getsockopt,
    gid_t,
    in_addr_t,
    in_port_t,
    ioctl,
    lchown,
    link,
    // iovec,
    listen,
    mkdir,
    mode_t,
    msghdr,
    nice,
    off_t,
    open,
    pid_t,
    pread,
    pwrite,
    // readv,
    recvfrom,
    recvmsg,
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
    stat,
    symlink,
    uid_t,
    uname,
    utsname,
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

use crate::utils;
#[allow(unused_imports)]
use std::io::Error;
use std::mem;

// Linking to functions that are not provided by rust libc
#[cfg(target_os = "macos")]
#[link(name = "c")]
extern "C" {
    pub fn wait4(pid: pid_t, status: *mut c_int, options: c_int, rusage: *mut rusage) -> pid_t;
    pub fn madvise(addr: *mut c_void, len: size_t, advice: c_int) -> c_int;
    pub fn fdatasync(fd: c_int) -> c_int;
    pub fn lstat64(path: *const c_char, buf: *mut c_void) -> c_int;
}

#[cfg(not(target_os = "macos"))]
use libc::{fallocate, fdatasync, ftruncate64, lstat, madvise, wait4};

// Another conditional constant for name resolution: Macos et iOS use
// SO_NOSIGPIPE as a setsockopt flag to disable SIGPIPE emission on socket.
// Other platforms do otherwise.
#[cfg(target_os = "darwin")]
use libc::SO_NOSIGPIPE;
#[cfg(not(target_os = "darwin"))]
const SO_NOSIGPIPE: c_int = 0;

/// open
pub fn ___syscall5(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let flags: i32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    let _path_str = unsafe { std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap() };
    let fd = unsafe { open(pathname_addr, flags, mode) };
    debug!(
        "=> pathname: {}, flags: {}, mode: {} = fd: {}\npath: {}\nlast os error: {}",
        pathname,
        flags,
        mode,
        fd,
        _path_str,
        Error::last_os_error(),
    );
    fd
}

/// link
pub fn ___syscall9(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall9 (link) {}", _which);

    let oldname: c_int = varargs.get(ctx);
    let newname: c_int = varargs.get(ctx);
    let oldname_ptr = emscripten_memory_pointer!(ctx.memory(0), oldname) as *const i8;
    let newname_ptr = emscripten_memory_pointer!(ctx.memory(0), newname) as *const i8;
    let result = unsafe { link(oldname_ptr, newname_ptr) };
    debug!(
        "=> oldname: {}, newname: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(oldname_ptr).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(newname_ptr).to_str().unwrap() },
        result,
    );
    result
}

/// getrusage
pub fn ___syscall77(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall77 (getrusage) {}", _which);

    let resource: c_int = varargs.get(ctx);
    let rusage_ptr: c_int = varargs.get(ctx);
    #[allow(clippy::cast_ptr_alignment)]
    let rusage = emscripten_memory_pointer!(ctx.memory(0), rusage_ptr) as *mut rusage;
    assert_eq!(8, mem::align_of_val(&rusage));
    unsafe { getrusage(resource, rusage) }
}

/// symlink
pub fn ___syscall83(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall83 (symlink) {}", _which);

    let path1_ptr: c_int = varargs.get(ctx);
    let path2_ptr: c_int = varargs.get(ctx);
    let path1 = emscripten_memory_pointer!(ctx.memory(0), path1_ptr) as *mut i8;
    let path2 = emscripten_memory_pointer!(ctx.memory(0), path2_ptr) as *mut i8;
    let result = unsafe { symlink(path1, path2) };
    debug!(
        "=> path1: {}, path2: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path1).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(path2).to_str().unwrap() },
        result,
    );
    result
}

/// ftruncate64
pub fn ___syscall194(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall194 (ftruncate64) {}", _which);
    let _fd: c_int = varargs.get(ctx);
    let _length: i64 = varargs.get(ctx);
    #[cfg(not(target_os = "macos"))]
    unsafe {
        ftruncate64(_fd, _length)
    }
    #[cfg(target_os = "macos")]
    unimplemented!()
}

/// lchown
pub fn ___syscall198(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall198 (lchown) {}", _which);
    let path: c_int = varargs.get(ctx);
    let uid: uid_t = varargs.get(ctx);
    let gid: gid_t = varargs.get(ctx);
    let path_ptr = emscripten_memory_pointer!(ctx.memory(0), path) as *const i8;
    let result = unsafe { lchown(path_ptr, uid, gid) };
    debug!(
        "=> path: {}, uid: {}, gid: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path_ptr).to_str().unwrap() },
        uid,
        gid,
        result,
    );
    result
}

/// getgroups
pub fn ___syscall205(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall205 (getgroups) {}", _which);
    let ngroups_max: c_int = varargs.get(ctx);
    let groups: c_int = varargs.get(ctx);

    #[allow(clippy::cast_ptr_alignment)]
    let gid_ptr = emscripten_memory_pointer!(ctx.memory(0), groups) as *mut gid_t;
    assert_eq!(4, mem::align_of_val(&gid_ptr));
    let result = unsafe { getgroups(ngroups_max, gid_ptr) };
    debug!(
        "=> ngroups_max: {}, gid_ptr: {:?}, result: {}",
        ngroups_max, gid_ptr, result,
    );
    result
}

// chown
pub fn ___syscall212(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", _which);

    let pathname: u32 = varargs.get(ctx);
    let owner: u32 = varargs.get(ctx);
    let group: u32 = varargs.get(ctx);

    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;

    unsafe { chown(pathname_addr, owner, group) }
}

/// madvise
pub fn ___syscall219(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall212 (chown) {}", _which);

    let addr_ptr: c_int = varargs.get(ctx);
    let len: usize = varargs.get(ctx);
    let advice: c_int = varargs.get(ctx);

    let addr = emscripten_memory_pointer!(ctx.memory(0), addr_ptr) as *mut c_void;

    unsafe { madvise(addr, len, advice) }
}

/// access
pub fn ___syscall33(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall33 (access) {}", _which);
    let path_ptr: c_int = varargs.get(ctx);
    let amode: c_int = varargs.get(ctx);
    let path = emscripten_memory_pointer!(ctx.memory(0), path_ptr) as *const i8;
    let result = unsafe { access(path, amode) };
    debug!(
        "=> path: {}, amode: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(path).to_str().unwrap() },
        amode,
        result
    );
    result
}

/// nice
pub fn ___syscall34(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall34 (nice) {}", _which);
    let inc_r: c_int = varargs.get(ctx);
    unsafe { nice(inc_r) }
}

// mkdir
pub fn ___syscall39(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    unsafe { mkdir(pathname_addr, mode as _) }
}

/// dup
pub fn ___syscall41(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall41 (dup) {}", _which);
    let fd: c_int = varargs.get(ctx);
    unsafe { dup(fd) }
}

/// getgid
pub fn ___syscall200(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall200 (getgid)");
    unsafe { getgid() as i32 }
}

// getgid
pub fn ___syscall201(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall201 (getgid)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid() as i32
    }
}

// getgid32
pub fn ___syscall202(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getgid32)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getgid() as _
    }
}

/// fchown
pub fn ___syscall207(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall207 (fchown) {}", _which);
    let fd: c_int = varargs.get(ctx);
    let owner: uid_t = varargs.get(ctx);
    let group: gid_t = varargs.get(ctx);
    unsafe { fchown(fd, owner, group) }
}

/// dup3
pub fn ___syscall330(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> pid_t {
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

/// ioctl
pub fn ___syscall54(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let request: u32 = varargs.get(ctx);
    debug!("=> fd: {}, op: {}", fd, request);
    // Got the equivalents here: https://code.woboq.org/linux/linux/include/uapi/asm-generic/ioctls.h.html
    // let argp: u32 = varargs.get(ctx);
    // let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
    // let ret = unsafe { ioctl(fd, request as _, argp_ptr) };
    // debug!("=> {}", ret);
    // ret
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

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", _which);
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
            fd as _
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
            let _proper_address = address as *const GuestSockaddrIn;
            debug!(
                    "=> address.sin_family: {:?}, address.sin_port: {:?}, address.sin_addr.s_addr: {:?}",
                unsafe { (*_proper_address).sin_family }, unsafe { (*_proper_address).sin_port }, unsafe { (*_proper_address).sin_addr.s_addr }
                );

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

            fd as _
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

// pread
pub fn ___syscall180(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", _which);
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
pub fn ___syscall181(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", _which);
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

/// fchmod
pub fn ___syscall94(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fchmod) {}", _which);
    let fd: c_int = varargs.get(ctx);
    let mode: mode_t = varargs.get(ctx);
    unsafe { fchmod(fd, mode) }
}

/// wait4
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall114(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> pid_t {
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

/// fsync
pub fn ___syscall118(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall118 (fsync) {}", _which);
    let fd: c_int = varargs.get(ctx);
    unsafe { fsync(fd) }
}

// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall142 (newselect) {}", _which);

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

/// fdatasync
pub fn ___syscall148(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall148 (fdatasync) {}", _which);

    let fd: i32 = varargs.get(ctx);

    unsafe { fdatasync(fd) }
}

// setpgid
pub fn ___syscall57(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall57 (setpgid) {}", _which);
    let pid: i32 = varargs.get(ctx);
    let pgid: i32 = varargs.get(ctx);
    unsafe { setpgid(pid, pgid) }
}

/// uname
// NOTE: Wondering if we should return custom utsname, like Emscripten.
pub fn ___syscall122(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall122 (uname) {}", _which);
    let buf: u32 = varargs.get(ctx);
    debug!("=> buf: {}", buf);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut utsname;
    unsafe { uname(buf_addr) }
}

/// lstat64
pub fn ___syscall196(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall196 (lstat64) {}", _which);
    let path_ptr: c_int = varargs.get(ctx);
    let buf_ptr: u32 = varargs.get(ctx);
    let path = emscripten_memory_pointer!(ctx.memory(0), path_ptr) as *const i8;
    unsafe {
        let mut stat: stat = std::mem::zeroed();

        #[cfg(target_os = "macos")]
        let stat_ptr = &mut stat as *mut stat as *mut c_void;
        #[cfg(not(target_os = "macos"))]
        let stat_ptr = &mut stat as *mut stat;

        #[cfg(target_os = "macos")]
        let ret = lstat64(path, stat_ptr);
        #[cfg(not(target_os = "macos"))]
        let ret = lstat(path, stat_ptr);

        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        utils::copy_stat_into_wasm(ctx, buf_ptr, &stat);
    }
    0
}

/// fallocate
pub fn ___syscall324(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall324 (fallocate) {}", _which);
    let _fd: c_int = varargs.get(ctx);
    let _mode: c_int = varargs.get(ctx);
    let _offset: off_t = varargs.get(ctx);
    let _len: off_t = varargs.get(ctx);
    #[cfg(not(target_os = "macos"))]
    unsafe {
        fallocate(_fd, _mode, _offset, _len)
    }
    #[cfg(target_os = "macos")]
    {
        unimplemented!()
    }
}
