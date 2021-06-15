use crate::blocking::types::{SockaddrIn, AF_INET};
use std::mem;
use std::net::{Ipv4Addr, SocketAddrV4};

impl From<&SocketAddrV4> for SockaddrIn {
    fn from(value: &SocketAddrV4) -> Self {
        SockaddrIn {
            sin_family: AF_INET as _,
            sin_port: value.port().to_be(),
            sin_addr: value.ip().octets(),
            ..unsafe { mem::zeroed() }
        }
    }
}

impl Into<SocketAddrV4> for SockaddrIn {
    fn into(self) -> SocketAddrV4 {
        let [o, p, q, r] = self.sin_addr;

        SocketAddrV4::new(Ipv4Addr::new(o, p, q, r), u16::from_be(self.sin_port))
    }
}
