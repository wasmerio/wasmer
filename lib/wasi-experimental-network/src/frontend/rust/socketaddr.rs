use crate::types::{__wasi_socket_address_in_t, __wasi_socket_address_t, AF_INET};
use std::net::{Ipv4Addr, SocketAddrV4};

impl From<&SocketAddrV4> for __wasi_socket_address_t {
    fn from(value: &SocketAddrV4) -> Self {
        __wasi_socket_address_t {
            v4: __wasi_socket_address_in_t {
                family: AF_INET,
                address: value.ip().octets(),
                port: value.port().to_be(),
            },
        }
    }
}

impl Into<SocketAddrV4> for __wasi_socket_address_in_t {
    fn into(self) -> SocketAddrV4 {
        let [o, p, q, r] = self.address;

        SocketAddrV4::new(Ipv4Addr::new(o, p, q, r), u16::from_be(self.port))
    }
}
