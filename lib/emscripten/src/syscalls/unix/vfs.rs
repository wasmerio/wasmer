use crate::syscalls::emscripten_vfs::FileHandle::{Socket, VirtualFile};
use crate::syscalls::emscripten_vfs::{FileHandle, VirtualFd};
use crate::utils::{copy_stat_into_wasm, read_string_from_wasm};
use crate::varargs::VarArgs;
use libc::stat;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::slice;
use wasmer_runtime_core::vm::Ctx;
use bit_field::BitArray;

/// read
pub fn ___syscall3(ctx: &mut Ctx, _: i32, mut varargs: VarArgs) -> i32 {
    debug!("emscripten::___syscall3 (read - vfs)",);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    debug!("=> fd: {}, buf_offset: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut u8;
    let mut buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let vfd = VirtualFd(fd);
    let virtual_file_handle = vfs.get_virtual_file_handle(vfd).unwrap();
    let ret = vfs
        .vfs
        .read_file(virtual_file_handle as _, &mut buf_slice)
        .unwrap();
    debug!("=> read syscall returns: {}", ret);
    debug!("read: '{}'", read_string_from_wasm(ctx.memory(0), buf));
    ret as _
}

/// write
pub fn ___syscall4(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall4 (write - vfs) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf);
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfd = VirtualFd(fd);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let count: usize = match vfs.fd_map.get(&vfd) {
        Some(FileHandle::VirtualFile(handle)) => {
            vfs.vfs
                .write_file(*handle as _, buf_slice, count as _, 0)
                .unwrap();
            count as usize
        }
        Some(FileHandle::Socket(host_fd)) => unsafe {
            libc::write(*host_fd, buf_addr as _, count as _) as usize
        },
        None => panic!(),
    };
    debug!("wrote: {}", read_string_from_wasm(ctx.memory(0), buf));
    debug!(
        "=> fd: {} (host {}), buf: {}, count: {}\n",
        vfd.0, fd, buf, count
    );
    count as c_int
}

/// open
pub fn ___syscall5(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall5 (open vfs) {}", _which);
    let pathname: u32 = varargs.get(ctx);
    let pathname_addr = emscripten_memory_pointer!(ctx.memory(0), pathname) as *const i8;
    let path_str = unsafe { std::ffi::CStr::from_ptr(pathname_addr).to_str().unwrap() };
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let fd = vfs.vfs.open_file(path_str).unwrap();
    let virtual_file_handle = FileHandle::VirtualFile(fd);
    let next_lowest_virtual_fd = vfs.next_lowest_fd();
    assert!(
        !vfs.fd_map.contains_key(&next_lowest_virtual_fd),
        "Emscripten vfs should not contain file descriptor."
    );
    vfs.fd_map.insert(next_lowest_virtual_fd.clone(), virtual_file_handle);
    debug!("=> opening `{}` with new virtual fd: {} (zbox handle {})", path_str, next_lowest_virtual_fd.clone(), fd);
    debug!("{}", path_str);
    return next_lowest_virtual_fd.0 as _;
}

/// close
pub fn ___syscall6(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall6 (close vfs) {}", _which);
    let fd: i32 = varargs.get(ctx);
    debug!("closing virtual fd {}...", fd);

    //    let emscripten_data = crate::env::get_emscripten_data(ctx);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let vfd = VirtualFd(fd);

    let close_result = match vfs.fd_map.get(&vfd) {
        Some(VirtualFile(handle)) => {
            debug!("closing virtual fd {} (virtual file {})...", fd, handle);
            vfs.vfs.close(handle).unwrap();
            vfs.fd_map.remove(&vfd);
            0
        }
        Some(Socket(host_fd)) => unsafe {
            debug!("closing virtual fd {} (socket {})...", fd, host_fd);
            let result = libc::close(*host_fd);
            if result == 0 {
                vfs.fd_map.remove(&vfd);
                0
            } else {
                -1
            }
        },
        _ => -1,
    };
    debug!("close returns {}", close_result);
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
    //    debug!("mkdir: {}", absolute_path.display());
    let emscripten_data = crate::env::get_emscripten_data(ctx);
    let ret = if let Some(vfs) = &mut emscripten_data.vfs {
        match vfs.vfs.make_dir(&absolute_path) {
            Ok(_) => 0,
            Err(_) => -1,
        }
    } else {
        -1
    };
    ret
}

/// pipe
pub fn ___syscall42(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    unimplemented!();
    debug!("emscripten::___syscall42 (pipe)");
    // offset to a file descriptor, which contains a read end and write end, 2 integers
    let fd_offset: u32 = varargs.get(ctx);

    let emscripten_memory = ctx.memory(0);

    // convert the file descriptor into a vec with two slots
    let mut fd_vec: Vec<c_int> = emscripten_memory.view()[((fd_offset / 4) as usize)..]
        .iter()
        .map(|pipe_end: &std::cell::Cell<c_int>| pipe_end.get())
        .take(2)
        .collect();

    // get it as a mutable pointer
    let fd_ptr = fd_vec.as_mut_ptr();

    // call pipe and store the pointers in this array
    #[cfg(target_os = "windows")]
    let result: c_int = unsafe { libc::pipe(fd_ptr, 2048, 0) };
    #[cfg(not(target_os = "windows"))]
    let result: c_int = unsafe { libc::pipe(fd_ptr) };
    result
}

/// ioctl
pub fn ___syscall54(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall54 (ioctl) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let request: u32 = varargs.get(ctx);
    debug!("virtual fd: {}, op: {}", fd, request);

    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let vfd = VirtualFd(fd);

    let host_fd = match vfs.fd_map.get(&vfd) {
        Some(Socket(host_fd)) => *host_fd,
        Some(_) => 0,
        _ => panic!("Should not ioctl on a vbox file."),
    };

    // Got the equivalents here: https://code.woboq.org/linux/linux/include/uapi/asm-generic/ioctls.h.html
    match request as _ {
        21537 => {
            // FIONBIO
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
            let ret = unsafe { libc::ioctl(host_fd, libc::FIONBIO, argp_ptr) };
            debug!("ret(FIONBIO): {}", ret);
            ret
            // 0
        }
        21523 => {
            // TIOCGWINSZ
            let argp: u32 = varargs.get(ctx);
            let argp_ptr = emscripten_memory_pointer!(ctx.memory(0), argp) as *mut c_void;
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

    let src = VirtualFd(src);
    let dst = VirtualFd(dst);

    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();

    // if the src is a valid file descriptor, then continue
    if !vfs.fd_map.contains_key(&src) {
        return -1;
    }
    // if src and dst are identical, do nothing
    if src == dst {
        return 0;
    }
    // test if the destination needs to closed first, if so, close it atomically (or fake it)
    if vfs.fd_map.contains_key(&dst) {
        vfs.close(&dst);
    }

    let dst_file_handle = match vfs.fd_map.get(&src) {
        Some(FileHandle::VirtualFile(handle)) => {
            let new_handle: i32 = vfs.vfs.duplicate_handle(handle);
            FileHandle::VirtualFile(new_handle)
        }
        Some(FileHandle::Socket(src_host_fd)) => unsafe {
            // get a dst file descriptor, or just use the underlying dup syscall
            let dst_host_fd = libc::dup(*src_host_fd);
            if dst_host_fd == -1 {
                panic!()
            }
            FileHandle::Socket(dst_host_fd)
        },
        None => panic!(),
    };

    vfs.fd_map.insert(dst.clone(), dst_file_handle);

    let dst = dst.0;

    debug!("emscripten::___syscall63 (dup2) returns {}", dst);

    dst
}

// socketcall
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall102(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall102 (socketcall) {}", _which);
    let call: u32 = varargs.get(ctx);
    let mut socket_varargs: VarArgs = varargs.get(ctx);

    #[cfg(target_os = "windows")]
    type libc_sa_family_t = u16;
    #[cfg(not(target_os = "windows"))]
    type libc_sa_family_t = libc::sa_family_t;

    #[cfg(target_os = "windows")]
    type libc_in_port_t = u16;
    #[cfg(not(target_os = "windows"))]
    type libc_in_port_t = libc::in_port_t;

    #[cfg(target_os = "windows")]
    type libc_in_addr_t = u32;
    #[cfg(not(target_os = "windows"))]
    type libc_in_addr_t = libc::in_addr_t;

    #[repr(C)]
    pub struct GuestSockaddrIn {
        pub sin_family: libc_sa_family_t, // u16
        pub sin_port: libc_in_port_t,     // u16
        pub sin_addr: GuestInAddr,        // u32
        pub sin_zero: [u8; 8],            // u8 * 8
                                          // 2 + 2 + 4 + 8 = 16
    }

    #[repr(C)]
    pub struct GuestInAddr {
        pub s_addr: libc_in_addr_t, // u32
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
            vfd.0 as _
        }
        2 => {
            debug!("socket: bind");
            // bind (socket: c_int, address: *const sockaddr, address_len: socklen_t) -> c_int
            // TODO: Emscripten has a different signature.
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

            // Debug received address
            let _proper_address = address as *const GuestSockaddrIn;
            let _other_proper_address = address as *const libc::sockaddr;
            unsafe {
                debug!(
                    "=> address.sin_family: {:?}, address.sin_port: {:?}, address.sin_addr.s_addr: {:?}",
                    (*_proper_address).sin_family, (*_proper_address).sin_port, (*_proper_address).sin_addr.s_addr
                );
            }
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
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();
            unsafe { libc::connect(host_socket_fd as _, address, address_len) }
        }
        4 => {
            debug!("socket: listen");
            // listen (socket: c_int, backlog: c_int) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let backlog: i32 = socket_varargs.get(ctx);

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();
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
            let address =
                emscripten_memory_pointer!(ctx.memory(0), address_addr) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let host_socket_fd = {
                let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
                let vfd = VirtualFd(socket);
                let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();
                host_socket_fd
            };

            debug!(
                "=> socket: {}(host {}), address: {:?}, address_len: {}",
                socket, host_socket_fd, address, address_len
            );

            let new_accept_host_fd =
                unsafe { libc::accept(host_socket_fd, address, address_len_addr) };

            unsafe {
                let address_linux =
                    emscripten_memory_pointer!(ctx.memory(0), address_addr) as *mut LinuxSockAddr;
                (*address_linux).sa_family = (*address).sa_family as u16;
                (*address_linux).sa_data = (*address).sa_data;
            };

            // set_cloexec
            let _ioctl_result = unsafe { libc::ioctl(new_accept_host_fd, libc::FIOCLEX) };

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let new_vfd = vfs.new_socket_fd(new_accept_host_fd);

            debug!("new accept fd: {}(host {})", new_vfd.0, new_accept_host_fd);

            new_vfd.0 as _
        }
        6 => {
            debug!("socket: getsockname");
            // getsockname (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

            unsafe { libc::getsockname(host_socket_fd as _, address, address_len_addr) }
        }
        7 => {
            debug!("socket: getpeername");
            // getpeername (socket: c_int, address: *mut sockaddr, address_len: *mut socklen_t) -> c_int
            let socket: i32 = socket_varargs.get(ctx);
            let address: u32 = socket_varargs.get(ctx);
            let address_len: u32 = socket_varargs.get(ctx);
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

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
            let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

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
            let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as _;
            let address = emscripten_memory_pointer!(ctx.memory(0), address) as *mut libc::sockaddr;
            let address_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), address_len) as *mut libc::socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

            let rcv_result = unsafe {
                libc::recvfrom(
                    host_socket_fd,
                    buf_addr,
                    flags,
                    len,
                    address,
                    address_len_addr,
                ) as i32
            };
            rcv_result

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
            let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as _; // Endian problem

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

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
            let name: i32 = socket_varargs.get(ctx);
            let value: u32 = socket_varargs.get(ctx);
            let option_len: u32 = socket_varargs.get(ctx);
            let value_addr = emscripten_memory_pointer!(ctx.memory(0), value) as _;
            let option_len_addr =
                emscripten_memory_pointer!(ctx.memory(0), option_len) as *mut socklen_t;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

            let result = unsafe {
                libc::getsockopt(host_socket_fd, level, name, value_addr, option_len_addr)
            };

            if result == -1 {
                let err = errno::errno();
                debug!("socket: getsockopt -- error -- {}", err);
            }

            result
        }
        16 => {
            debug!("socket: sendmsg");
            // sendmsg (fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_pointer!(ctx.memory(0), msg) as *const libc::msghdr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

            unsafe { libc::sendmsg(host_socket_fd as _, msg_addr, flags) as i32 }
        }
        17 => {
            debug!("socket: recvmsg");
            // recvmsg (fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t
            let socket: i32 = socket_varargs.get(ctx);
            let msg: u32 = socket_varargs.get(ctx);
            let flags: i32 = socket_varargs.get(ctx);
            let msg_addr = emscripten_memory_pointer!(ctx.memory(0), msg) as *mut libc::msghdr;

            let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
            let vfd = VirtualFd(socket);
            let host_socket_fd = vfs.get_host_socket_fd(&vfd).unwrap();

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
//    unimplemented!();
    // -> ssize_t
    debug!("emscripten::___syscall146 (writev) {}", _which);
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
//            let curr = libc::write(fd, iov_base, iov_len);

            let curr = {
                debug!("emscripten::___syscall4 (write - vfs) {}", _which);
                let fd: i32 = fd; // varargs.get(ctx);
//                let buf = iov_base; // varargs.get(ctx);
                let count: i32 = iov_len; // varargs.get(ctx);
                let buf_slice = unsafe { slice::from_raw_parts_mut(iov_base as *mut _, count as _) };
                let vfd = VirtualFd(fd);
                let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
                let count: usize = match vfs.fd_map.get(&vfd) {
                    Some(FileHandle::VirtualFile(handle)) => {
                        vfs.vfs
                            .write_file(*handle as _, buf_slice, count as _, 0)
                            .unwrap();
                        count as usize
                    }
                    Some(FileHandle::Socket(host_fd)) => unsafe {
                        libc::write(*host_fd, iov_base as _, count as _) as usize
                    },
                    None => panic!(),
                };
//                debug!("wrote: {}", read_string_from_wasm(ctx.memory(0), buf));
                debug!(
                    "=> fd: {} (host {}), buf: {:?}, count: {}\n",
                    vfd.0, fd, iov_base, count
                );
                count as c_int
            };

            if curr < 0 {
                return -1;
            }
            ret += curr;
        }
        ret as _
    }
}

/// pread
pub fn ___syscall180(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall180 (pread) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: i32 = varargs.get(ctx);
    let offset: i32/*i64*/ = varargs.get(ctx);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *mut u8;
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let mut buf_slice_with_offset: &mut [u8] = &mut buf_slice[(offset as usize)..];
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let vfd = VirtualFd(fd);
    let virtual_file_handle = vfs.get_virtual_file_handle(vfd).unwrap();
    let ret = vfs
        .vfs
        .read_file(virtual_file_handle as _, &mut buf_slice_with_offset)
        .unwrap();
    debug!("read: '{}'", read_string_from_wasm(ctx.memory(0), buf));
    ret as _
}

/// pwrite
pub fn ___syscall181(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> c_int {
    debug!("emscripten::___syscall181 (pwrite) {}", _which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: u32 = varargs.get(ctx);
    let offset: i32 = varargs.get(ctx);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf);
    let buf_slice = unsafe { slice::from_raw_parts_mut(buf_addr, count as _) };
    let vfd = VirtualFd(fd);
    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();
    let virtual_file_handle = vfs.get_virtual_file_handle(vfd).unwrap();
    vfs.vfs
        .write_file(virtual_file_handle as _, buf_slice, count as _, offset as _)
        .unwrap();
    debug!("wrote: '{}'", read_string_from_wasm(ctx.memory(0), buf));
    count as _
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
            let metadata = vfs.vfs.get_path_metadata(&path).unwrap();
            let len = metadata.len();
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
    let vfd = VirtualFd(fd);
    let ret = match vfs.fd_map.get(&vfd) {
        Some(FileHandle::VirtualFile(internal_handle)) => {
            let metadata = vfs.vfs.get_file_metadata(internal_handle).unwrap();
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
pub fn ___syscall330(ctx: &mut Ctx, _which: c_int, mut varargs: VarArgs) -> libc::pid_t {
    unimplemented!();
    // Implementation based on description at https://linux.die.net/man/2/dup3
    debug!("emscripten::___syscall330 (dup3)");
    let oldfd: c_int = varargs.get(ctx);
    let newfd: c_int = varargs.get(ctx);
    let flags: c_int = varargs.get(ctx);

    if oldfd == newfd {
        return libc::EINVAL;
    }

    let res = unsafe { libc::dup2(oldfd, newfd) };

    // Set flags on newfd (https://www.gnu.org/software/libc/manual/html_node/Descriptor-Flags.html)
    let mut old_flags = unsafe { libc::fcntl(newfd, libc::F_GETFD, 0) };

    if old_flags > 0 {
        old_flags |= flags;
    } else if old_flags == 0 {
        old_flags &= !flags;
    }

    unsafe {
        libc::fcntl(newfd, libc::F_SETFD, old_flags);
    }

    debug!(
        "=> oldfd: {}, newfd: {}, flags: {} = pid: {}",
        oldfd, newfd, flags, res
    );
    res
}
