use crate::syscalls::emscripten_vfs::FileHandle;
use crate::utils::{copy_stat_into_wasm, read_string_from_wasm};
use crate::varargs::VarArgs;
use libc::stat;
use std::cell::RefCell;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::slice;
use wasmer_runtime_core::memory::Memory;
use wasmer_runtime_core::vm::Ctx;

#[inline]
pub fn emscripten_memory_ptr(memory: &Memory, offset: u32) -> *mut u8 {
    use std::cell::Cell;
    (&memory.view::<u8>()[(offset as usize)..]).as_ptr() as *mut Cell<u8> as *mut u8
}

/// read
pub fn ___syscall3(ctx: &mut Ctx, _: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall3 (read - vfs)",);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf) as *mut u8;
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let ret = vfs.read_file(fd, buf_slice);
    debug!("=> read syscall returns: {}", ret);
    ret as _
}

/// write
pub fn ___syscall4(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall4 (write - vfs) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf);
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    match vfs.write_file(fd, buf_slice, count as usize) {
        Ok(count) => count as _,
        Err(_) => -1,
    }
}

/// open
pub fn ___syscall5(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open vfs) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_ptr(ctx.memory(0), pathname) as *const i8;
    let path_str = unsafe { std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap() };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let fd = vfs.open_file(path_str);
    fd
}

/// close
pub fn ___syscall6(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close vfs) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let close_result = vfs.close_file_descriptor(fd);
    close_result
}

/// chmod
pub fn ___syscall15(_ctx: &mut Ctx, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::___syscall15 (chmod)");
    debug!("chmod always returns 0.");
    0
}

/// mkdir
pub fn ___syscall39(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall39 (mkdir vfs) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let _mode: u32 = varargs.get(ctx);
    let path = read_string_from_wasm(ctx.memory(0), pathname);
    let root = std::path::PathBuf::from("/");
    let absolute_path = root.join(&path);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    vfs.make_dir(&absolute_path);
    0
}

/// pipe
pub fn ___syscall42(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> c_int {
    unimplemented!("emscripten::___syscall42 (pipe)");
}

/// ioctl
pub fn ___syscall54(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let request: u32 = varargs.get(ctx);
    debug!("virtual fd: {}, op: {}", fd, request);

    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let host_fd = match vfs.get_host_socket_fd(fd) {
        Some(host_fd) => host_fd,
        _ => return -1,
    };

    // Got the equivalents here: https://code.woboq.org/linux/linux/include/uapi/asm-generic/ioctls.h.html
    match request as _ {
        21537 => {
            // FIONBIO
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_ptr(ctx.memory(0), argp) as *mut c_void;
            let ret = unsafe { libc::ioctl(host_fd, libc::FIONBIO, argp_ptr) };
            debug!("ret(FIONBIO): {}", ret);
            ret
            // 0
        }
        21523 => {
            // TIOCGWINSZ
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_ptr(ctx.memory(0), argp) as *mut c_void;
            let ret = unsafe { libc::ioctl(host_fd, libc::TIOCGWINSZ, argp_ptr) };
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

/// dup2
pub fn ___syscall63(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall63 (dup2) {}", _which);
    let src: i32 = varargs.get(ctx);
    let dst: i32 = varargs.get(ctx);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    // if the src is a valid file descriptor, then continue
    if !vfs.fd_map.contains_key(&src) {
        return -1;
    }
    // if src and dst are identical, do nothing
    if src == dst {
        return 0;
    }
    let _ = vfs.fd_map.remove(&dst);
    let dst_file_handle = match vfs.fd_map.get(&src) {
        Some(FileHandle::Vf(file)) => FileHandle::Vf(file.clone()),
        Some(FileHandle::Socket(src_host_fd)) => {
            // get a dst file descriptor, or just use the underlying dup syscall
            match unsafe { libc::dup(*src_host_fd) } {
                -1 => return -1,
                dst_host_fd => FileHandle::Socket(dst_host_fd),
            }
        }
        None => return -1,
    };
    vfs.fd_map.insert(dst, dst_file_handle);
    debug!("emscripten::___syscall63 (dup2) returns {}", dst);
    dst
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", _which);
    let call: u32 = varargs.get(ctx);
    let mut socket_varargs: VarArgs = varargs.get(ctx);

    #[repr(C)]
    pub struct GuestSockaddrIn {
        pub sin_family: libc::sa_family_t, // u16
        pub sin_port: libc::in_port_t,     // u16
        pub sin_addr: GuestInAddr,         // u32
        pub sin_zero: [u8; 8],             // u8 * 8
                                           // 2 + 2 + 4 + 8 = 16
    }

    #[repr(C)]
    pub struct GuestInAddr {
        pub s_addr: libc::in_addr_t, // u32
    }

    pub struct LinuxSockAddr {
        pub sa_family: u16,
        pub sa_data: [libc::c_char; 14],
    }

    match call {
        1 => {
            debug!("socket: socket");
            // Another conditional constant for name resolution: Macos et iOS use
            // SO_NOSIGPIPE as a setsockopt flag to disable SIGPIPE emission on socket.
            // Other platforms do otherwise.
            #[cfg(target_os = "darwin")]
            use libc::SO_NOSIGPIPE;
            #[cfg(not(target_os = "darwin"))]
            const SO_NOSIGPIPE: c_int = 0;
            // socket (domain: c_int, ty: c_int, protocol: c_int) -> c_int
            let domain: i32 = socket_varargs.get(ctx);
            let ty: i32 = socket_varargs.get(ctx);
            let protocol: i32 = socket_varargs.get(ctx);
            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let _test_errno = errno::errno();
            // create the host socket
            let host_fd = unsafe { libc::socket(domain, ty, protocol) };
            let vfd = vfs.new_socket_fd(host_fd);
            // set_cloexec
            let _ioctl_result = unsafe { libc::ioctl(host_fd, libc::FIOCLEX) };
            let _err = errno::errno();
            let _result = unsafe {
                libc::setsockopt(host_fd, libc::SOL_SOCKET, SO_NOSIGPIPE, 0 as *const _, 4)
            };
            let _err2 = errno::errno();
            debug!(
                "=> domain: {} (AF_INET/2), type: {} (SOCK_STREAM/1), protocol: {} = fd: {}",
                domain, ty, protocol, vfd
            );
            vfd
        }
        2 => {
            debug!("socket: bind");
            // bind (socket: c_int, address: *const sockaddr, address_len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            // Debug received address
            let _proper_address = address as *const GuestSockaddrIn;
            let _other_proper_address = address as *const libc::sockaddr;
            debug!(
                "=> address.sin_family: {:?}, address.sin_port: {:?}, address.sin_addr.s_addr: {:?}",
                unsafe { (*_proper_address).sin_family }, unsafe { (*_proper_address).sin_port }, unsafe { (*_proper_address).sin_addr.s_addr }
            );
            let status = unsafe { libc::bind(host_socket_fd as _, address, address_len) };
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
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();
            unsafe { libc::connect(host_socket_fd as _, address, address_len) }
        }
        4 => {
            debug!("socket: listen");
            // listen (socket: c_int, backlog: c_int) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let backlog: i32 = socket_varargs.get(ctx);
            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();
            let status = unsafe { libc::listen(host_socket_fd, backlog) };
            debug!(
                "=> socketfd: {}, backlog: {} = status: {}",
                socket, backlog, status
            );
            status
        }
        5 => {
            debug!("socket: accept");
            let socket: i32 = socket_varargs.get(ctx);
            let address_addr: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_ptr(ctx.memory(0), address_addr) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_ptr(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let host_socket_fd = {
                let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
                let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();
                host_socket_fd
            };

            debug!(
                "=> socket: {}(host {}), address: {:?}, address_len: {}",
                socket, host_socket_fd, address, address_len
            );

            let new_accept_host_fd =
                unsafe { libc::accept(host_socket_fd, address, address_len_addr) };

            if new_accept_host_fd < 0 {
                panic!("accept file descriptor should not be negative.");
            }

            unsafe {
                let address_linux =
                    emscripten_memory_ptr(ctx.memory(0), address_addr) as *mut LinuxSockAddr;
                (*address_linux).sa_family = (*address).sa_family as u16;
                (*address_linux).sa_data = (*address).sa_data;
            };

            // set_cloexec
            let _ioctl_result = unsafe { libc::ioctl(new_accept_host_fd, libc::FIOCLEX) };

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let new_vfd = vfs.new_socket_fd(new_accept_host_fd);

            debug!("new accept fd: {}(host {})", new_vfd, new_accept_host_fd);

            new_vfd
        }
        6 => {
            debug!("socket: getsockname");
            // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_ptr(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            unsafe { libc::getsockname(host_socket_fd as _, address, address_len_addr) }
        }
        7 => {
            debug!("socket: getpeername");
            // getpeername (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_ptr(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            unsafe { libc::getpeername(host_socket_fd as _, address, address_len_addr) }
        }
        11 => {
            debug!("socket: sendto");
            // sendto (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let buf: u32 = socket_varargs.get(ctx);
            let flags = socket_varargs.get(ctx);
            let len: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf) as _;
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            unsafe {
                libc::sendto(
                    host_socket_fd as _,
                    buf_addr,
                    flags,
                    len,
                    address,
                    address_len,
                ) as i32
            }
        }
        12 => {
            debug!("socket: recvfrom");
            // recvfrom (socket: c_int, buf: *const c_void, len: size_t, flags: c_int, addr: *const sockaddr, addrlen: socklen_t) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let buf: u32 = socket_varargs.get(ctx);
            let flags = socket_varargs.get(ctx);
            let len: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf) as _;
            let address = emscripten_memory_ptr(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_ptr(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            let recv_result = unsafe {
                libc::recvfrom(
                    host_socket_fd,
                    buf_addr,
                    flags,
                    len,
                    address,
                    address_len_addr,
                ) as i32
            };
            debug!(
                "recvfrom: socket: {}, flags: {}, len: {}, result: {}",
                socket, flags, len, recv_result
            );
            recv_result
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
            let level: i32 = libc::SOL_SOCKET;
            let _: u32 = socket_varargs.get(ctx);
            // SO_REUSEADDR = 0x4 (BSD, Linux)
            let name: i32 = libc::SO_REUSEADDR;
            let _: u32 = socket_varargs.get(ctx);
            let value: u32 = socket_varargs.get(ctx);
            let option_len = socket_varargs.get(ctx);
            let value_addr = emscripten_memory_ptr(ctx.memory(0), value) as _; // Endian problem

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            let ret = unsafe {
                libc::setsockopt(host_socket_fd as _, level, name, value_addr, option_len)
            };

            debug!("=> socketfd: {}, level: {} (SOL_SOCKET/0xffff), name: {} (SO_REUSEADDR/4), value_addr: {:?}, option_len: {} = status: {}", socket, level, name, value_addr, option_len, ret);
            ret
        }
        15 => {
            debug!("socket: getsockopt");
            // getsockopt (sockfd: c_int, level: c_int, optname: c_int, optval: *mut c_void, optlen: *mut socklen_t) -> c_int
            use libc::socklen_t;
            let socket = socket_varargs.get(ctx);
            let level: i32 = socket_varargs.get(ctx);
            let correct_level = if level == 1 { libc::SOL_SOCKET } else { level };
            let name: i32 = socket_varargs.get(ctx);
            let correct_name = if name == 3 { libc::SO_TYPE } else { name };

            let value: u32 = socket_varargs.get(ctx);
            let option_len: u32 = socket_varargs.get(ctx);
            let value_addr = emscripten_memory_ptr(ctx.memory(0), value) as _;
            let option_len_addr =
                emscripten_memory_ptr(ctx.memory(0), option_len) as *mut socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = match vfs.get_host_socket_fd(socket) {
                Some(host_fd) => host_fd,
                None => {
                    return -1;
                }
            };
            let result = unsafe {
                libc::getsockopt(
                    host_socket_fd,
                    correct_level,
                    correct_name,
                    value_addr,
                    option_len_addr,
                )
            };
            result
        }
        16 => {
            debug!("socket: sendmsg");
            // sendmsg (fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_ptr(ctx.memory(0), msg) as *const libc::msghdr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            unsafe { libc::sendmsg(host_socket_fd as _, msg_addr, flags) as i32 }
        }
        17 => {
            debug!("socket: recvmsg");
            // recvmsg (fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_ptr(ctx.memory(0), msg) as *mut libc::msghdr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let host_socket_fd = vfs.get_host_socket_fd(socket).unwrap();

            unsafe { libc::recvmsg(host_socket_fd as _, msg_addr, flags) as i32 }
        }
        _ => {
            // others
            -1
        }
    }
}

/// writev
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall146(ctx: &mut Ctx, _which: i32, mut varargs: VarArgs) -> i32 {
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let iov_array_offset: i32 = varargs.get(ctx);
    let iovcnt: i32 = varargs.get(ctx);

    #[repr(C)]
    struct GuestIovec {
        iov_base: i32,
        iov_len: i32,
    }

    let mut err = false;
    let iov_array_offset = iov_array_offset as u32;
    let count = (0..iovcnt as u32).fold(0, |acc, iov_array_index| {
        let iov_offset = iov_array_offset + iov_array_index * 8; // get the offset to the iov
        let (iov_buf_slice, iov_buf_ptr, count) = {
            let emscripten_memory = ctx.memory(0);
            let guest_iov_ptr =
                emscripten_memory_ptr(emscripten_memory, iov_offset) as *mut GuestIovec;
            let iov_base_offset = unsafe { (*guest_iov_ptr).iov_base as u32 };
            let iov_buf_ptr =
                emscripten_memory_ptr(emscripten_memory, iov_base_offset) as *const c_void;
            let iov_len = unsafe { (*guest_iov_ptr).iov_len as usize };
            let iov_buf_slice = unsafe { slice::from_raw_parts(iov_buf_ptr as *const u8, iov_len) };
            (iov_buf_slice, iov_buf_ptr, iov_len)
        };
        let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
        let count: usize = match vfs.fd_map.get(&fd) {
            Some(FileHandle::Vf(file)) => match RefCell::borrow_mut(file).write(iov_buf_slice) {
                Ok(count) => count,
                _ => {
                    err = true;
                    0
                }
            },
            Some(FileHandle::Socket(host_fd)) => unsafe {
                let count = libc::write(*host_fd, iov_buf_ptr, count);
                count as usize
            },
            None => {
                err = true;
                0
            }
        };
        acc + count
    });

    if err {
        return -1;
    }

    debug!(
        "=> fd: {}, iov: {}, iovcnt = {}, returning {}",
        fd, iov_array_offset, iovcnt, count
    );
    count as _
}

/// pread
pub fn ___syscall180(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    let offset: i32/*i64*/ = varargs.get(ctx);
    let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf) as *mut u8;
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let mut buf_slice_with_offset: &mut [u8] = &mut buf_slice[(offset as usize)..];
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_ref().unwrap(); //.as_mut().unwrap();
    let ret = vfs.read_file(fd, &mut buf_slice_with_offset);
    debug!("read: '{}'", read_string_from_wasm(ctx.memory(0), buf));
    ret as _
}

/// pwrite
pub fn ___syscall181(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: u32 = varargs.get(ctx);
    let _offset: i32 = varargs.get(ctx);
    let buf_addr = emscripten_memory_ptr(ctx.memory(0), buf);
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    match vfs.write_file(fd, buf_slice, count as _) {
        Ok(count) => count as _,
        Err(e) => {
            eprintln!("{:?}", e);
            -1
        }
    }
}

// stat64
pub fn ___syscall195(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall195 (stat64) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let path_string = read_string_from_wasm(ctx.memory(0), pathname);
    debug!("path extract for `stat` syscall: {}", &path_string);
    let path = std::path::PathBuf::from(path_string);

    let emscripten_data = crate::env::get_emscripten_data(ctx);
    let ret = match &mut emscripten_data.vfs {
        Some(vfs) => {
            let metadata = vfs.path_metadata(&path).unwrap();
            let len = metadata.len;
            unsafe {
                let mut stat: stat = std::mem::zeroed();
                stat.st_size = len as _;
                debug!("stat size: {}", len);
                copy_stat_into_wasm(ctx, buf, &stat as _);
            }
            0
        }
        None => -1,
    };
    debug!("stat return: {}", ret);
    ret
}

/// fstat64
pub fn ___syscall197(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall197 (fstat64) {}", _which);
    let fd: c_int = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let ret = match vfs.fd_map.get(&fd) {
        Some(FileHandle::Vf(file)) => {
            let metadata = file.borrow_mut().metadata().unwrap();
            //            let metadata = vfs.vfs.get_file_metadata(internal_handle).unwrap();
            let len = metadata.len;
            let mode = if metadata.is_file {
                libc::S_IFREG
            } else {
                libc::S_IFDIR
            };
            unsafe {
                let mut stat: stat = std::mem::zeroed();
                stat.st_mode = mode as _;
                stat.st_size = len as _;
                debug!("fstat size: {}", len);
                copy_stat_into_wasm(ctx, buf, &stat as _);
            }
            0
        }
        Some(FileHandle::Socket(_host_fd)) => panic!(),
        None => -1,
    };
    debug!("fstat return: {}", ret);
    ret
}

/// dup3
pub fn ___syscall330(_ctx: &mut Ctx, _which: c_int, mut _varargs: VarArgs) -> libc::pid_t {
    unimplemented!();
}
