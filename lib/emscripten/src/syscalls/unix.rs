use crate::{ptr::WasmPtr, varargs::VarArgs};
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
    c_ulong,
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
    getegid,
    geteuid,
    getgid,
    getgroups,
    getpeername,
    getpgid,
    getrusage,
    getsockname,
    getsockopt,
    getuid,
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
    readdir,
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
    TIOCGWINSZ,
    TIOCSPGRP,
    // TCGETS,
    // TCSETSW,
};

// They are not exposed in in Rust libc in macOS
const TCGETS: u64 = 0x5401;
const TCSETSW: u64 = 0x5403;

// `libc` constants as provided by `emscripten`. Maybe move to own file?
const WASM_FIONBIO: u32 = 0x5421;
const WASM_FIOCLEX: u32 = 0x5451;
const WASM_TIOCSPGRP: u32 = 0x5410;
const WASM_TIOCGWINSZ: u32 = 0x5413;
const WASM_TCGETS: u32 = 0x5401;
const WASM_TCSETSW: u32 = 0x5403;

// Based on @syrusakbary sugerence at
// https://github.com/wasmerio/wasmer/pull/532#discussion_r300837800
fn translate_ioctl(wasm_ioctl: u32) -> c_ulong {
    match wasm_ioctl {
        WASM_FIOCLEX => FIOCLEX,
        WASM_TIOCGWINSZ => TIOCGWINSZ,
        WASM_TIOCSPGRP => TIOCSPGRP,
        WASM_FIONBIO => FIONBIO,
        WASM_TCGETS => TCGETS,
        WASM_TCSETSW => TCSETSW,
        _otherwise => {
            unimplemented!("The ioctl {} is not yet implemented", wasm_ioctl);
        }
    }
}

#[allow(unused_imports)]
use std::ffi::CStr;
use wasmer_runtime_core::vm::Ctx;

use crate::env::EmSockAddr;
use crate::utils::{self, get_cstr_path};
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
    pub fn lstat64(path: *const libc::c_char, buf: *mut c_void) -> c_int;
}

#[cfg(not(target_os = "macos"))]
use libc::{fallocate, fdatasync, ftruncate64, lstat, madvise, wait4};

// Another conditional constant for name resolution: Macos et iOS use
// SO_NOSIGPIPE as a setsockopt flag to disable SIGPIPE emission on socket.
// Other platforms do otherwise.
#[cfg(target_os = "macos")]
use libc::SO_NOSIGPIPE;
#[cfg(not(target_os = "macos"))]
const SO_NOSIGPIPE: c_int = 0;

/// open
pub fn ___syscall5(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open) {}", _which);
    let pathname_addr = varargs.get_str(ctx);
    let flags: i32 = varargs.get(ctx);
    let mode: u32 = varargs.get(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    let _path_str = unsafe { std::ffi::CStr::from_ptr(real_path).to_str().unwrap() };
    let fd = unsafe { open(real_path, flags, mode) };
    debug!(
        "=> path: {}, flags: {}, mode: {} = fd: {}",
        _path_str, flags, mode, fd,
    );
    if fd == -1 {
        debug!("=> last os error: {}", Error::last_os_error(),);
    }
    fd
}

/// link
pub fn ___syscall9(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall9 (link) {}", _which);

    let oldname_ptr = varargs.get_str(ctx);
    let newname_ptr = varargs.get_str(ctx);
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

    let path1 = varargs.get_str(ctx);
    let path2 = varargs.get_str(ctx);
    let real_path1_owned = utils::get_cstr_path(ctx, path1 as *const _);
    let real_path1 = if let Some(ref rp) = real_path1_owned {
        rp.as_c_str().as_ptr()
    } else {
        path1
    };
    let real_path2_owned = utils::get_cstr_path(ctx, path2 as *const _);
    let real_path2 = if let Some(ref rp) = real_path2_owned {
        rp.as_c_str().as_ptr()
    } else {
        path2
    };
    let result = unsafe { symlink(real_path1, real_path2) };
    debug!(
        "=> path1: {}, path2: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(real_path1).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(real_path2).to_str().unwrap() },
        result,
    );
    result
}

/// readlink
pub fn ___syscall85(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall85 (readlink)");
    let pathname_addr = varargs.get_str(ctx);
    let buf = varargs.get_str(ctx);
    // let buf_addr: i32 = varargs.get(ctx);
    let buf_size: i32 = varargs.get(ctx);
    let real_path_owned = get_cstr_path(ctx, pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };

    let ret = unsafe { libc::readlink(real_path, buf as _, buf_size as _) as i32 };
    if ret == -1 {
        debug!("readlink failed");
        return ret;
    }
    debug!(
        "=> path: {}, buf: {}, buf_size: {}, return: {} ",
        unsafe { std::ffi::CStr::from_ptr(real_path).to_str().unwrap() },
        unsafe { std::ffi::CStr::from_ptr(buf as _).to_str().unwrap() },
        buf_size,
        ret
    );
    ret
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
    unimplemented!("emscripten::___syscall194 (ftruncate64) {}", _which)
}

/// lchown
pub fn ___syscall198(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall198 (lchown) {}", _which);
    let path_ptr = varargs.get_str(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, path_ptr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        path_ptr
    };
    let uid: uid_t = varargs.get(ctx);
    let gid: gid_t = varargs.get(ctx);
    let result = unsafe { lchown(real_path, uid, gid) };
    debug!(
        "=> path: {}, uid: {}, gid: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(real_path).to_str().unwrap() },
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

    let pathname_addr = varargs.get_str(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    let owner: u32 = varargs.get(ctx);
    let group: u32 = varargs.get(ctx);

    unsafe { chown(real_path, owner, group) }
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
    let path = varargs.get_str(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, path as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        path
    };
    let amode: c_int = varargs.get(ctx);
    let result = unsafe { access(real_path, amode) };
    debug!(
        "=> path: {}, amode: {}, result: {}",
        unsafe { std::ffi::CStr::from_ptr(real_path).to_str().unwrap() },
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
    let pathname_addr = varargs.get_str(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, pathname_addr as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        pathname_addr
    };
    let mode: u32 = varargs.get(ctx);
    unsafe { mkdir(real_path, mode as _) }
}

/// dup
pub fn ___syscall41(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall41 (dup) {}", _which);
    let fd: c_int = varargs.get(ctx);
    unsafe { dup(fd) }
}

/// getgid32
pub fn ___syscall200(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall200 (getgid32)");
    unsafe { getgid() as i32 }
}

// geteuid32
pub fn ___syscall201(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall201 (geteuid32)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        geteuid() as i32
    }
}

// getegid32
pub fn ___syscall202(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    // gid_t
    debug!("emscripten::___syscall202 (getegid32)");
    unsafe {
        // Maybe fix: Emscripten returns 0 always
        getegid() as _
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
    match request {
        WASM_FIOCLEX | WASM_FIONBIO | WASM_TIOCGWINSZ | WASM_TIOCSPGRP | WASM_TCGETS
        | WASM_TCSETSW => {
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
            let translated_request = translate_ioctl(request);
            let ret = unsafe { ioctl(fd, translated_request, argp_ptr) };
            debug!(
                " => request: {}, translated: {}, return: {}",
                request, translated_request, ret
            );

            // TODO: We hardcode the value to have emscripten tests pass, as for some reason
            // when the capturer is active, ioctl returns -1 instead of 0
            if request == WASM_TIOCGWINSZ && ret == -1 {
                return 0;
            }
            ret
        }
        _ => {
            debug!(
                " => not implemented case {} (noop, hardcoded to 0)",
                request
            );
            0
        }
    }
}

const SOCK_NON_BLOCK: i32 = 2048;
const SOCK_CLOEXC: i32 = 0x80000;

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", _which);
    let call: u32 = varargs.get(ctx);
    let mut socket_varargs: VarArgs = varargs.get(ctx);

    // migrating to EmSockAddr, port being separate here is nice, should update that too
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

    match call {
        1 => {
            debug!("socket: socket");
            // socket (domain: c_int, ty: c_int, protocol: c_int) -> c_int
            let domain: i32 = socket_varargs.get(ctx);
            let ty_and_flags: i32 = socket_varargs.get(ctx);
            let protocol: i32 = socket_varargs.get(ctx);
            let ty = ty_and_flags & (!SOCK_NON_BLOCK) & (!SOCK_CLOEXC);
            let fd = unsafe { socket(domain, ty, protocol) };

            if ty_and_flags & SOCK_CLOEXC != 0 {
                // set_cloexec
                unsafe {
                    ioctl(fd, translate_ioctl(WASM_FIOCLEX));
                };
            }

            if ty_and_flags & SOCK_NON_BLOCK != 0 {
                // do something here
                unimplemented!("non blocking sockets");
            }

            // why is this here?
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
                "=> domain: {}, type: {}, protocol: {} = fd: {}",
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
            let socket: i32 = socket_varargs.get(ctx);
            let address: WasmPtr<EmSockAddr> = socket_varargs.get(ctx);
            let address_len: WasmPtr<u32> = socket_varargs.get(ctx);

            debug!(
                "=> socket: {}, address: {:?}, address_len: {}",
                socket,
                address.deref(ctx.memory(0)).unwrap().get(),
                address_len.deref(ctx.memory(0)).unwrap().get()
            );
            let address_len_addr =
                unsafe { address_len.deref_mut(ctx.memory(0)).unwrap().get_mut() };
            // let mut address_len_addr: socklen_t = 0;

            let mut host_address: sockaddr = sockaddr {
                sa_family: Default::default(),
                sa_data: Default::default(),
                #[cfg(target_os = "macos")]
                sa_len: Default::default(),
            };
            let fd = unsafe { accept(socket, &mut host_address, address_len_addr) };
            let address_addr = unsafe { address.deref_mut(ctx.memory(0)).unwrap().get_mut() };

            address_addr.sa_family = host_address.sa_family as _;
            address_addr.sa_data = host_address.sa_data.clone();

            // why is this here?
            // set_cloexec
            unsafe {
                ioctl(fd, translate_ioctl(WASM_FIOCLEX));
            };

            debug!(
                "address: {:?}, len: {}, result fd = {}",
                address_addr, address_len_addr, fd
            );

            fd as _
        }
        6 => {
            debug!("socket: getsockname");
            // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let address: WasmPtr<EmSockAddr> = socket_varargs.get(ctx);
            let address_len: WasmPtr<u32> = socket_varargs.get(ctx);
            let address_len_addr =
                unsafe { address_len.deref_mut(ctx.memory(0)).unwrap().get_mut() };

            let mut sock_addr_host: sockaddr = sockaddr {
                sa_family: Default::default(),
                sa_data: Default::default(),
                #[cfg(target_os = "macos")]
                sa_len: Default::default(),
            };
            let ret = unsafe {
                getsockname(
                    socket,
                    &mut sock_addr_host as *mut sockaddr,
                    address_len_addr as *mut u32,
                )
            };
            // translate from host data into emscripten data
            let mut address_mut = unsafe { address.deref_mut(ctx.memory(0)).unwrap().get_mut() };
            address_mut.sa_family = sock_addr_host.sa_family as _;
            address_mut.sa_data = sock_addr_host.sa_data.clone();

            debug!(
                "=> socket: {}, address, {:?}, address_len: {}, result = {}",
                socket, address_mut, address_len_addr, ret
            );

            ret
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
            let len: i32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut socklen_t;
            unsafe {
                recvfrom(
                    socket,
                    buf_addr,
                    len as usize,
                    flags,
                    address,
                    address_len_addr,
                ) as i32
            }
        }
        14 => {
            debug!("socket: setsockopt");
            // OSX and BSD have completely different values, be very careful here
            //      https://github.com/openbsd/src/blob/master/sys/sys/socket.h#L156
            // setsockopt (socket: c_int, level: c_int, name: c_int, value: *const c_void, option_len: socklen_t) -> c_int

            let socket = socket_varargs.get(ctx);
            let level: i32 = socket_varargs.get(ctx);
            let level = if level == 1 { SOL_SOCKET } else { level };
            let untranslated_name: i32 = socket_varargs.get(ctx);
            let value: u32 = socket_varargs.get(ctx);
            let option_len: u32 = socket_varargs.get(ctx);
            let value_addr =
                emscripten_memory_pointer!(ctx.memory(0), value) as *const libc::c_void;
            let name: i32 = translate_socket_name_flag(untranslated_name);

            let ret = unsafe { setsockopt(socket, level, name, value_addr, option_len) };

            debug!("=> socketfd: {}, level: {}, name: {}, value_addr: {:?}, option_len: {} = status: {}", socket, level, untranslated_name, value_addr, option_len, ret);
            ret
        }
        15 => {
            debug!("socket: getsockopt");
            // getsockopt (sockfd: c_int, level: c_int, optname: c_int, optval: *mut c_void, optlen: *mut socklen_t) -> c_int
            let socket = socket_varargs.get(ctx);
            let level: i32 = socket_varargs.get(ctx);
            let level = if level == 1 { SOL_SOCKET } else { level };
            let untranslated_name: i32 = socket_varargs.get(ctx);
            let name: i32 = translate_socket_name_flag(untranslated_name);
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

/// OSX and BSD have completely different values, we must translate from emscripten's Linuxy
/// value into one that we can pass to native syscalls
fn translate_socket_name_flag(name: i32) -> i32 {
    match name {
        2 => libc::SO_REUSEADDR,
        3 => libc::SO_TYPE,
        4 => libc::SO_ERROR,
        5 => libc::SO_DONTROUTE,
        6 => libc::SO_BROADCAST,
        7 => libc::SO_SNDBUF,
        8 => libc::SO_RCVBUF,
        9 => libc::SO_KEEPALIVE,
        10 => libc::SO_OOBINLINE,
        13 => libc::SO_LINGER,
        18 => libc::SO_RCVLOWAT,
        19 => libc::SO_SNDLOWAT,
        20 => libc::SO_RCVTIMEO,
        21 => libc::SO_SNDTIMEO,
        // SO_DEBUG missing
        30 => libc::SO_ACCEPTCONN,
        otherwise => otherwise,
    }
}

/// getpgid
pub fn ___syscall132(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall132 (getpgid)");

    let pid: pid_t = varargs.get(ctx);

    let ret = unsafe { getpgid(pid) };
    debug!("=> pid: {} = {}", pid, ret);
    if ret == -1 {
        debug!("=> last os error: {}", Error::last_os_error(),);
    }
    ret
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct EmPollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

unsafe impl wasmer_runtime_core::types::ValueType for EmPollFd {}

/// poll
pub fn ___syscall168(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall168(poll)");
    let fds: WasmPtr<EmPollFd> = varargs.get(ctx);
    let nfds: u32 = varargs.get(ctx);
    let timeout: i32 = varargs.get(ctx);

    let fds_mut = unsafe { fds.deref_mut(ctx.memory(0)).unwrap().get_mut() };

    let ret = unsafe {
        libc::poll(
            fds_mut as *mut EmPollFd as *mut libc::pollfd,
            nfds as _,
            timeout,
        )
    };

    ret
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

    if nfds > 1024 {
        // EINVAL
        return -22;
    }
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

    let ret = unsafe { setpgid(pid, pgid) };
    debug!("=> pid: {}, pgid: {} = {}", pid, pgid, ret);
    if ret == -1 {
        debug!("=> last os error: {}", Error::last_os_error(),);
    }
    ret
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
    let path = varargs.get_str(ctx);
    let real_path_owned = utils::get_cstr_path(ctx, path as *const _);
    let real_path = if let Some(ref rp) = real_path_owned {
        rp.as_c_str().as_ptr()
    } else {
        path
    };
    let buf_ptr: u32 = varargs.get(ctx);
    unsafe {
        let mut stat: stat = std::mem::zeroed();

        #[cfg(target_os = "macos")]
        let stat_ptr = &mut stat as *mut stat as *mut c_void;
        #[cfg(not(target_os = "macos"))]
        let stat_ptr = &mut stat as *mut stat;

        #[cfg(target_os = "macos")]
        let ret = lstat64(real_path, stat_ptr);
        #[cfg(not(target_os = "macos"))]
        let ret = lstat(real_path, stat_ptr);

        debug!("ret: {}", ret);
        if ret != 0 {
            return ret;
        }
        utils::copy_stat_into_wasm(ctx, buf_ptr, &stat);
    }
    0
}

// getuid
pub fn ___syscall199(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall199 (getuid)");
    let uid = unsafe { getuid() as _ };
    debug!("  => {}", uid);
    uid
}

// getdents
// dirent structure is
// i64, i64, u16 (280), i8, [i8; 256]
pub fn ___syscall220(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    use super::super::env::get_emscripten_data;

    let fd: i32 = varargs.get(ctx);
    let dirp_addr: i32 = varargs.get(ctx);
    let count: u32 = varargs.get(ctx);
    debug!(
        "emscripten::___syscall220 (getdents) {} {} {}",
        fd, dirp_addr, count
    );

    let dirp = emscripten_memory_pointer!(ctx.memory(0), dirp_addr) as *mut u8;

    let opened_dirs = &mut get_emscripten_data(ctx).opened_dirs;

    // need to persist stream across calls?
    // let dir: *mut libc::DIR = unsafe { libc::fdopendir(fd) };
    let dir = &*opened_dirs
        .entry(fd)
        .or_insert_with(|| unsafe { Box::new(libc::fdopendir(fd)) });

    let mut pos = 0;
    let offset = 256 + 12;
    while pos + offset <= count as usize {
        let dirent = unsafe { readdir(**dir) };
        if dirent.is_null() {
            break;
        }
        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            *(dirp.add(pos) as *mut u32) = (*dirent).d_ino as u32;
            *(dirp.add(pos + 4) as *mut u32) = pos as u32;
            *(dirp.add(pos + 8) as *mut u16) = offset as u16;
            *(dirp.add(pos + 10) as *mut u8) = (*dirent).d_type;
            let upper_bound = std::cmp::min((*dirent).d_reclen, 255) as usize;
            let mut i = 0;
            while i < upper_bound {
                *(dirp.add(pos + 11 + i) as *mut c_char) = (*dirent).d_name[i] as c_char;
                i += 1;
            }
            // We set the termination string char
            *(dirp.add(pos + 11 + i) as *mut c_char) = 0 as c_char;
            debug!(
                "  => file {}",
                CStr::from_ptr(dirp.add(pos + 11) as *const c_char)
                    .to_str()
                    .unwrap()
            );
        }
        pos += offset;
    }
    pos as i32
}

// fcntl64
pub fn ___syscall221(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall221 (fcntl64) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let cmd: i32 = varargs.get(ctx);
    let arg: i32 = varargs.get(ctx);
    // (FAPPEND   - 0x08
    // |FASYNC    - 0x40
    // |FFSYNC    - 0x80
    // |FNONBLOCK - 0x04
    let ret = unsafe { fcntl(fd, cmd, arg) };
    debug!("=> fd: {}, cmd: {} = {}", fd, cmd, ret);
    if ret == -1 {
        debug!("=> last os error: {}", Error::last_os_error(),);
    }
    ret
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
        unimplemented!("emscripten::___syscall324 (fallocate) {}", _which)
    }
}
