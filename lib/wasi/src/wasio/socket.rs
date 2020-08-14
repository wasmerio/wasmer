use super::types::*;

/// An IPv4 socket.
pub const AF_INET: __wasio_socket_domain_t = 2;

/// An IPv6 socket.
pub const AF_INET6: __wasio_socket_domain_t = 10;

/// Provides sequenced, reliable, two-way, connection-based byte streams.
///
/// Implies TCP when used with an IP socket.
pub const SOCK_STREAM: __wasio_socket_type_t = 1;

/// Supports datagrams (connectionless, unreliable messages of a fixed maximum length).
///
/// Implies UDP when used with an IP socket.
pub const SOCK_DGRAM: __wasio_socket_type_t = 2;
