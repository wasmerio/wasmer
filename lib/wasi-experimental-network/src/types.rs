#![allow(non_camel_case_types)]

use std::fmt;
use std::mem;
pub use wasmer_wasi_types::*;

/// The `sockaddr_in` struct.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SockaddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

impl SockaddrIn {
    pub fn size_of_self(&self) -> u32 {
        mem::size_of::<Self>() as u32
    }
}

impl fmt::Debug for SockaddrIn {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "{}.{}.{}.{}:{}",
            self.sin_addr[0],
            self.sin_addr[1],
            self.sin_addr[2],
            self.sin_addr[3],
            u16::from_be(self.sin_port),
        )
    }
}

/// The `sockaddr_in6` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct SockaddrIn6 {
    pub sin6_family: u16,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

impl SockaddrIn6 {
    pub fn size_of_self(&self) -> u32 {
        mem::size_of::<Self>() as u32
    }
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

pub type __wasi_shutdown_t = i32;

pub const SHUT_RD: __wasi_shutdown_t = 1;
pub const SHUT_WR: __wasi_shutdown_t = 2;
pub const SHUT_RDWR: __wasi_shutdown_t = 3;
