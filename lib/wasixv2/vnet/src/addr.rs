//! Address helpers shared by the port tables and sockets.
//!
//! Internally an address is kept *canonical*: an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) is
//! stored as the IPv4 address it maps, because Linux handles such a socket as an IPv4 one for
//! lookups and conflicts. Each socket converts back to its own family when it reports an
//! address to the guest.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::Domain;

/// Domain-related helpers.
impl Domain {
    /// `AF_INET` / `AF_INET6`.
    pub fn raw(self) -> i32 {
        match self {
            Self::Inet => 2,
            Self::Inet6 => 10,
        }
    }
}

/// Maps `::ffff:a.b.c.d` to `a.b.c.d`; everything else is unchanged.
pub fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => ip,
        },
        v4 => v4,
    }
}

pub fn canonical(addr: SocketAddr) -> SocketAddr {
    SocketAddr::new(canonical_ip(addr.ip()), addr.port())
}

/// Presents a canonical address the way a socket of `family` reports it.
pub fn present(family: Domain, addr: SocketAddr) -> SocketAddr {
    match (family, addr) {
        (Domain::Inet6, SocketAddr::V4(v4)) => {
            SocketAddr::V6(SocketAddrV6::new(v4.ip().to_ipv6_mapped(), v4.port(), 0, 0))
        }
        _ => addr,
    }
}

/// The address an unbound socket of `family` reports from `getsockname`.
pub fn unbound(family: Domain) -> SocketAddr {
    match family {
        Domain::Inet => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        Domain::Inet6 => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
    }
}

/// `connect` to the wildcard address means the loopback address (`ip_route_connect`).
pub fn connect_target(addr: SocketAddr) -> SocketAddr {
    let ip = match canonical_ip(addr.ip()) {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, addr.port())
}

/// `sockaddr_in` or `sockaddr_in6` in guest layout, as a socket of `family` reports `addr`
/// (presented, so an IPv6 socket sees an IPv4 address mapped); `None` is the zeroed address
/// of `AF_UNSPEC`, in the same size (`SO_EE_OFFENDER` of an error no node reported).
pub fn sockaddr_bytes(family: Domain, addr: Option<SocketAddr>) -> Vec<u8> {
    let mut raw = vec![
        0;
        match family {
            Domain::Inet => 16,
            Domain::Inet6 => 28,
        }
    ];
    let Some(addr) = addr else {
        return raw;
    };
    match (family, present(family, canonical(addr))) {
        (Domain::Inet, SocketAddr::V4(a)) => {
            raw[..2].copy_from_slice(&(Domain::Inet.raw() as u16).to_le_bytes());
            raw[2..4].copy_from_slice(&a.port().to_be_bytes());
            raw[4..8].copy_from_slice(&a.ip().octets());
        }
        (Domain::Inet6, SocketAddr::V6(a)) => {
            raw[..2].copy_from_slice(&(Domain::Inet6.raw() as u16).to_le_bytes());
            raw[2..4].copy_from_slice(&a.port().to_be_bytes());
            raw[4..8].copy_from_slice(&a.flowinfo().to_be_bytes());
            raw[8..24].copy_from_slice(&a.ip().octets());
            raw[24..28].copy_from_slice(&a.scope_id().to_le_bytes());
        }
        // An IPv4 socket never holds an IPv6 address.
        _ => {}
    }
    raw
}
