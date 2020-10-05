//! Types used for WASIO APIs.
#![allow(non_camel_case_types)]

/// The cancellation token of an ongoing asynchronous operation.
#[derive(Debug)]
#[repr(transparent)]
pub struct CancellationToken(pub u64);

/// The user context that will be returned to WebAssembly, once a
/// requested asynchronous operation is completed.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct UserContext(pub u64);

/// The `sockaddr_in` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn {
    pub sin_family: i16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

/// The `sockaddr_in6` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn6 {
    pub sin6_family: i16,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

/// The type of a WASIO socket domain.
pub type __wasio_socket_domain_t = i32;

/// An IPv4 socket.
pub const AF_INET: __wasio_socket_domain_t = 2;

/// An IPv6 socket.
pub const AF_INET6: __wasio_socket_domain_t = 10;

/// The type of a WASIO socket type.
pub type __wasio_socket_type_t = i32;

/// Provides sequenced, reliable, two-way, connection-based byte streams.
///
/// Implies TCP when used with an IP socket.
pub const SOCK_STREAM: __wasio_socket_type_t = 1;

/// Supports datagrams (connectionless, unreliable messages of a fixed maximum length).
///
/// Implies UDP when used with an IP socket.
pub const SOCK_DGRAM: __wasio_socket_type_t = 2;

/// The type of a WASIO socket protocol.
pub type __wasio_socket_protocol_t = i32;

// Start of original WASI types.
// We should change these to libstd types if possible.

/// Error number.
pub type __wasi_errno_t = u16;
pub const __WASI_ESUCCESS: u16 = 0;
pub const __WASI_E2BIG: u16 = 1;
pub const __WASI_EACCES: u16 = 2;
pub const __WASI_EADDRINUSE: u16 = 3;
pub const __WASI_EADDRNOTAVAIL: u16 = 4;
pub const __WASI_EAFNOSUPPORT: u16 = 5;
pub const __WASI_EAGAIN: u16 = 6;
pub const __WASI_EALREADY: u16 = 7;
pub const __WASI_EBADF: u16 = 8;
pub const __WASI_EBADMSG: u16 = 9;
pub const __WASI_EBUSY: u16 = 10;
pub const __WASI_ECANCELED: u16 = 11;
pub const __WASI_ECHILD: u16 = 12;
pub const __WASI_ECONNABORTED: u16 = 13;
pub const __WASI_ECONNREFUSED: u16 = 14;
pub const __WASI_ECONNRESET: u16 = 15;
pub const __WASI_EDEADLK: u16 = 16;
pub const __WASI_EDESTADDRREQ: u16 = 17;
pub const __WASI_EDOM: u16 = 18;
pub const __WASI_EDQUOT: u16 = 19;
pub const __WASI_EEXIST: u16 = 20;
pub const __WASI_EFAULT: u16 = 21;
pub const __WASI_EFBIG: u16 = 22;
pub const __WASI_EHOSTUNREACH: u16 = 23;
pub const __WASI_EIDRM: u16 = 24;
pub const __WASI_EILSEQ: u16 = 25;
pub const __WASI_EINPROGRESS: u16 = 26;
pub const __WASI_EINTR: u16 = 27;
pub const __WASI_EINVAL: u16 = 28;
pub const __WASI_EIO: u16 = 29;
pub const __WASI_EISCONN: u16 = 30;
pub const __WASI_EISDIR: u16 = 31;
pub const __WASI_ELOOP: u16 = 32;
pub const __WASI_EMFILE: u16 = 33;
pub const __WASI_EMLINK: u16 = 34;
pub const __WASI_EMSGSIZE: u16 = 35;
pub const __WASI_EMULTIHOP: u16 = 36;
pub const __WASI_ENAMETOOLONG: u16 = 37;
pub const __WASI_ENETDOWN: u16 = 38;
pub const __WASI_ENETRESET: u16 = 39;
pub const __WASI_ENETUNREACH: u16 = 40;
pub const __WASI_ENFILE: u16 = 41;
pub const __WASI_ENOBUFS: u16 = 42;
pub const __WASI_ENODEV: u16 = 43;
pub const __WASI_ENOENT: u16 = 44;
pub const __WASI_ENOEXEC: u16 = 45;
pub const __WASI_ENOLCK: u16 = 46;
pub const __WASI_ENOLINK: u16 = 47;
pub const __WASI_ENOMEM: u16 = 48;
pub const __WASI_ENOMSG: u16 = 49;
pub const __WASI_ENOPROTOOPT: u16 = 50;
pub const __WASI_ENOSPC: u16 = 51;
pub const __WASI_ENOSYS: u16 = 52;
pub const __WASI_ENOTCONN: u16 = 53;
pub const __WASI_ENOTDIR: u16 = 54;
pub const __WASI_ENOTEMPTY: u16 = 55;
pub const __WASI_ENOTRECOVERABLE: u16 = 56;
pub const __WASI_ENOTSOCK: u16 = 57;
pub const __WASI_ENOTSUP: u16 = 58;
pub const __WASI_ENOTTY: u16 = 59;
pub const __WASI_ENXIO: u16 = 60;
pub const __WASI_EOVERFLOW: u16 = 61;
pub const __WASI_EOWNERDEAD: u16 = 62;
pub const __WASI_EPERM: u16 = 63;
pub const __WASI_EPIPE: u16 = 64;
pub const __WASI_EPROTO: u16 = 65;
pub const __WASI_EPROTONOSUPPORT: u16 = 66;
pub const __WASI_EPROTOTYPE: u16 = 67;
pub const __WASI_ERANGE: u16 = 68;
pub const __WASI_EROFS: u16 = 69;
pub const __WASI_ESPIPE: u16 = 70;
pub const __WASI_ESRCH: u16 = 71;
pub const __WASI_ESTALE: u16 = 72;
pub const __WASI_ETIMEDOUT: u16 = 73;
pub const __WASI_ETXTBSY: u16 = 74;
pub const __WASI_EXDEV: u16 = 75;
pub const __WASI_ENOTCAPABLE: u16 = 76;

/// File descriptor.
pub type __wasi_fd_t = u32;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct __wasi_ciovec_t {
    pub buf: *mut u8,
    pub buf_len: u32,
}

unsafe impl Send for __wasi_ciovec_t {}
unsafe impl Sync for __wasi_ciovec_t {}

pub type __wasi_siflags_t = u16;
pub type __wasi_riflags_t = u16;
pub type __wasi_roflags_t = u16;
