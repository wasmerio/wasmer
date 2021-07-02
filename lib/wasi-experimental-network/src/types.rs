#![allow(non_camel_case_types)]

use std::fmt;
use wasmer_types::ValueType;
pub use wasmer_wasi_types::*;

/// A type with the same memory layout as `libc::sockaddr`.
/// An union around `sockaddr_in` and `sockaddr_in6`.
#[repr(C)]
#[derive(Copy, Clone)]
pub union __wasi_socket_address_t {
    pub v4: __wasi_socket_address_in_t,
    pub v6: __wasi_socket_address_in6_t,
}

unsafe impl ValueType for __wasi_socket_address_t {}

impl fmt::Debug for __wasi_socket_address_t {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe {
            match self {
                Self {
                    v4:
                        v4
                        @
                        __wasi_socket_address_in_t {
                            family: AF_INET, ..
                        },
                } => write!(fmt, "{:?}", v4),
                _ => write!(fmt, "IPv6"),
            }
        }
    }
}

/// The `sockaddr_in` struct.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct __wasi_socket_address_in_t {
    pub family: __wasi_socket_domain_t,
    pub address: [u8; 4],
    pub port: u16,
}

impl fmt::Debug for __wasi_socket_address_in_t {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "{}.{}.{}.{}:{}",
            self.address[0],
            self.address[1],
            self.address[2],
            self.address[3],
            u16::from_be(self.port),
        )
    }
}

/// The `sockaddr_in6` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct __wasi_socket_address_in6_t {
    pub sin6_family: u16,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

/// The _domain_ specifies a communication domain; this selects the
/// protocol family which will be used for communication.
///
/// It uses `i32` which is the equivalent of `int` in C, which is the
/// typed used by `socket(2)` for the `domain` argument.
pub type __wasi_socket_domain_t = i32;

/// IPv4 Internet protocols.
pub const AF_INET: __wasi_socket_domain_t = 1;

/// IPv6 Internet protocols.
pub const AF_INET6: __wasi_socket_domain_t = 2;

pub const AF_UNIX: __wasi_socket_domain_t = 3;
pub const AF_PACKET: __wasi_socket_domain_t = 4;
pub const AF_VSOCK: __wasi_socket_domain_t = 5;

/// A socket has an indicated _type_, which specifies the
/// communication semantics.
///
/// It uses `i32` which is the equivalent of `int` in C, which is the
/// typed used by `socket(2)` for the `type` argument.
pub type __wasi_socket_type_t = i32;

/// Provides sequenced, reliable, two-way, connection-based byte
/// streams.
///
/// Implies TCP when used with an IP socket.
pub const SOCK_STREAM: __wasi_socket_type_t = 1;

/// Supports datagrams (connectionless, unreliable messages of a fixed
/// maximum length).
///
/// Implies UDP when used with an IP socket.
pub const SOCK_DGRAM: __wasi_socket_type_t = 2;

pub const SOCK_SEQPACKET: __wasi_socket_type_t = 3;
pub const SOCK_RAW: __wasi_socket_type_t = 4;

/// The _protocol_ specified a particular protocol to be used with the
/// socket. Normally only a single protocol exists to support a
/// particular socket type within a given protocol family, in which
/// case _protocol_ can be specified as 0 (or [`DEFAULT_PROTOCOL`]
/// here). However, it is possible that many protocols may exist, in
/// which case a particular protocol must be specified in this
/// manner. The protocol number to use is specific to the
/// “communication domain” in which communication is to take place.
///
/// It uses `i32` which is the equivalent of `int` in C, which is the
/// typed used by `socket(2)` for the `domain` argument.
pub type __wasi_socket_protocol_t = i32;

/// Represents the default protocol, i.e. `0`. See
/// [`__wasi_socket_protocol_t`] to learn more.
pub const DEFAULT_PROTOCOL: __wasi_socket_protocol_t = 0;
#[allow(non_upper_case_globals)]
pub const ICMPv4: __wasi_socket_protocol_t = 1;
#[allow(non_upper_case_globals)]
pub const ICMPv6: __wasi_socket_protocol_t = 2;
pub const TCP: __wasi_socket_protocol_t = 3;
pub const UDP: __wasi_socket_protocol_t = 4;

pub type __wasi_shutdown_t = i32;

pub const SHUT_RD: __wasi_shutdown_t = 1;
pub const SHUT_WR: __wasi_shutdown_t = 2;
pub const SHUT_RDWR: __wasi_shutdown_t = 3;

pub type __wasi_poll_t = u32;
pub type __wasi_poll_token_t = u32;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct __wasi_poll_event_t {
    pub token: __wasi_poll_token_t,
    pub readable: bool,
    pub writable: bool,
}

unsafe impl ValueType for __wasi_poll_event_t {}

#[cfg(always_fail)] // Rust will ignore it, but cbindgen will read it.
mod cbindgen {
    pub type __wasi_fd_t = u32;

    pub type __wasi_errno_t = u32;

    #[repr(C)]
    pub struct __wasi_ciovec_t {
        pub buf: u32,
        pub buf_len: u32,
    }

    #[repr(C)]
    pub struct __wasi_iovec_t {
        pub buf: u32,
        pub buf_len: u32,
    }

    pub type __wasi_siflags_t = u16;
}
