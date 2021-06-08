#![allow(non_camel_case_types)]

/// The `sockaddr_in` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

/// The `sockaddr_in6` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn6 {
    pub sin6_family: u16,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

/// The type of WASI socket domain.
pub type __wasi_socket_domain_t = u32;

/// An IPv4 socket.
pub const AF_INET: __wasi_socket_domain_t = 2;

/// An IPv6 socket.
pub const AF_INET6: __wasi_socket_domain_t = 10;

/// The type of WASI socket type.
pub type __wasi_socket_type_t = u32;

/// Provides sequenced, reliable, two-way, connection-based byte
/// streams.
///
/// Implies TCP when used with an IP socket.
pub const SOCK_STREAM: __wasi_socket_type_t = 1;

/// Supports datagrams (connection-less, unreliable messages of a
/// fixed maximum length).
///
/// Implies UDP when used with an IP socket.
pub const SOCK_DGRAM: __wasi_socket_type_t = 2;
