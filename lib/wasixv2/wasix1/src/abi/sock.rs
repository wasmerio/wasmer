//! Sockets, epoll and events (WASIX).
//!
//! wasix-libc passes its own `AF_*`, `SOCK_*` and `SO_*` numbers to the host, which are the v1
//! values below; the shims translate each to Linux's explicitly.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use super::{Fd, GuestLayout, Ptr, layout, newtype_layout};

/// `address_family` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addressfamily(pub u8);

impl Addressfamily {
    pub const UNSPEC: Self = Self(0);
    pub const INET4: Self = Self(1);
    pub const INET6: Self = Self(2);
    pub const UNIX: Self = Self(3);
}

/// `sock_type` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Socktype(pub u8);

impl Socktype {
    pub const UNUSED: Self = Self(0);
    pub const STREAM: Self = Self(1);
    pub const DGRAM: Self = Self(2);
    pub const RAW: Self = Self(3);
    pub const SEQPACKET: Self = Self(4);

    /// wasix-libc's `SOCK_NONBLOCK`, which its `socket()` leaves in the type argument.
    pub const LIBC_NONBLOCK: u32 = 0x4000;
    /// wasix-libc's `SOCK_CLOEXEC`, likewise.
    pub const LIBC_CLOEXEC: u32 = 0x2000;
}

/// `sock_status` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockStatus(pub u8);

impl SockStatus {
    pub const OPENING: Self = Self(0);
    pub const OPENED: Self = Self(1);
    pub const CLOSED: Self = Self(2);
    pub const FAILED: Self = Self(3);
}

/// `sock_option` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sockoption(pub u8);

impl Sockoption {
    pub const NOOP: Self = Self(0);
    pub const REUSE_PORT: Self = Self(1);
    pub const REUSE_ADDR: Self = Self(2);
    pub const NO_DELAY: Self = Self(3);
    pub const DONT_ROUTE: Self = Self(4);
    pub const ONLY_V6: Self = Self(5);
    pub const BROADCAST: Self = Self(6);
    pub const MULTICAST_LOOP_V4: Self = Self(7);
    pub const MULTICAST_LOOP_V6: Self = Self(8);
    pub const PROMISCUOUS: Self = Self(9);
    pub const LISTENING: Self = Self(10);
    pub const LAST_ERROR: Self = Self(11);
    pub const KEEP_ALIVE: Self = Self(12);
    pub const LINGER: Self = Self(13);
    pub const OOB_INLINE: Self = Self(14);
    pub const RECV_BUF_SIZE: Self = Self(15);
    pub const SEND_BUF_SIZE: Self = Self(16);
    pub const RECV_LOWAT: Self = Self(17);
    pub const SEND_LOWAT: Self = Self(18);
    pub const RECV_TIMEOUT: Self = Self(19);
    pub const SEND_TIMEOUT: Self = Self(20);
    pub const CONNECT_TIMEOUT: Self = Self(21);
    pub const ACCEPT_TIMEOUT: Self = Self(22);
    pub const TTL: Self = Self(23);
    pub const MULTICAST_TTL_V4: Self = Self(24);
    pub const TYPE: Self = Self(25);
    pub const PROTO: Self = Self(26);
    /// The highest option v1 defines.
    pub const MAX: Self = Self::PROTO;
}

/// `riflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiFlags(pub u16);

impl RiFlags {
    pub const RECV_PEEK: Self = Self(1 << 0);
    pub const RECV_WAITALL: Self = Self(1 << 1);
    pub const RECV_DATA_TRUNCATED: Self = Self(1 << 2);
    pub const RECV_DONT_WAIT: Self = Self(1 << 3);
}

/// `roflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoFlags(pub u16);

impl RoFlags {
    pub const RECV_DATA_TRUNCATED: Self = Self(1 << 0);
}

/// `siflags` (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiFlags(pub u16);

impl SiFlags {
    pub const SEND_DONT_WAIT: Self = Self(1 << 0);
}

/// `sdflags` (`u8`): the directions `sock_shutdown` closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sdflags(pub u8);

impl Sdflags {
    pub const RD: Self = Self(1 << 0);
    pub const WR: Self = Self(1 << 1);
}

/// `eventfdflags` (`u16`). wasix-libc truncates `eventfd`'s flags to 16 bits, so its
/// `EFD_NONBLOCK` (`O_NONBLOCK`, the fdflags bit) arrives here and `EFD_CLOEXEC` does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eventfdflags(pub u16);

impl Eventfdflags {
    pub const SEMAPHORE: Self = Self(1 << 0);
    pub const NONBLOCK: Self = Self(1 << 2);
}

/// `epoll_type` (`u32`): v1's own bit numbering, translated explicitly to Linux's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpollType(pub u32);

impl EpollType {
    pub const EPOLLIN: Self = Self(1 << 0);
    pub const EPOLLOUT: Self = Self(1 << 1);
    pub const EPOLLRDHUP: Self = Self(1 << 2);
    pub const EPOLLPRI: Self = Self(1 << 3);
    pub const EPOLLERR: Self = Self(1 << 4);
    pub const EPOLLHUP: Self = Self(1 << 5);
    pub const EPOLLET: Self = Self(1 << 6);
    pub const EPOLLONESHOT: Self = Self(1 << 7);
}

/// `epoll_ctl` (`u32`). The witx numbering (ADD 0, MOD 1, DEL 2) is not what the guest sends:
/// wasix-libc passes musl's Linux values through, and the v1 host decoded those, keeping 0 as
/// a second ADD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpollCtl(pub u32);

impl EpollCtl {
    /// The witx `ADD`, which the v1 host still accepted.
    pub const ADD_WITX: Self = Self(0);
    pub const ADD: Self = Self(1);
    pub const DEL: Self = Self(2);
    pub const MOD: Self = Self(3);
}

newtype_layout!(
    Addressfamily(u8),
    Socktype(u8),
    SockStatus(u8),
    Sockoption(u8),
    RiFlags(u16),
    RoFlags(u16),
    SiFlags(u16),
    Sdflags(u8),
    Eventfdflags(u16),
    EpollType(u32),
    EpollCtl(u32),
);

layout! {
    /// `addr_port`: a tagged union of an address family and an address with a port. The
    /// payload starts at 2: for inet4 and inet6 the port as a host-order `u16`, then the address
    /// in network order (inet6 then also has its flow info and scope id); for unix the path.
    /// Decode it with [`AddrPort::decode`].
    pub struct AddrPort (size 110, align 2) {
        pub tag: Addressfamily = 0,
        pub payload: [u8; 108] = 2,
    }
}

layout! {
    /// `addr`: an address without a port, laid out as [`AddrPort`] without the port.
    pub struct Addr (size 110, align 2) {
        pub tag: Addressfamily = 0,
        pub payload: [u8; 108] = 2,
    }
}

layout! {
    /// `addr_ip`: an IP address as `resolve` returns it, the address bytes at 2 (inet4 in the
    /// first four).
    pub struct AddrIp (size 18, align 2) {
        pub tag: Addressfamily = 0,
        pub octets: [u8; 16] = 2,
    }
}

layout! {
    /// `addr_cidr`: the address as in [`Addr`], then the prefix length (at 6 for inet4, at 26
    /// for inet6).
    pub struct AddrCidr (size 28, align 2) {
        pub tag: Addressfamily = 0,
        pub payload: [u8; 26] = 2,
    }
}

layout! {
    /// `addr_ip6`, a multicast group: the address, then flow info and scope id.
    pub struct AddrIp6 (size 24, align 2) {
        pub octets: [u8; 16] = 0,
        pub flow_info: [u8; 4] = 16,
        pub scope_id: [u8; 4] = 20,
    }
}

layout! {
    /// `hardware_address`.
    pub struct HardwareAddress (size 6, align 1) {
        pub octets: [u8; 6] = 0,
    }
}

/// `addr_ip4`: an IPv4 address in network order.
pub type AddrIp4 = [u8; 4];

layout! {
    /// `epoll_data`: wider than Linux's `epoll_data_t`; the compat layer keeps it host-side.
    pub struct EpollData (size 24, align 8) {
        pub ptr: Ptr = 0,
        pub fd: Fd = 4,
        pub data1: u32 = 8,
        pub data2: u64 = 16,
    }
}

layout! {
    /// `epoll_event`.
    pub struct EpollEvent (size 32, align 8) {
        pub events: EpollType = 0,
        pub data: EpollData = 8,
    }
}

/// A decoded [`AddrPort`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SockAddr {
    Inet(SocketAddr),
    /// The path, up to its first NUL.
    Unix(Vec<u8>),
    /// `UNSPEC`: no address.
    Unspec,
}

/// Offsets in the payload of the inet variants of [`AddrPort`]. inet6's flow info and scope id
/// are `u32`s stored as two host-order `u16` halves, high half first.
mod inet {
    pub const PORT: usize = 0;
    pub const ADDR: usize = 2;
    pub const FLOW_INFO: usize = 18;
    pub const SCOPE_ID: usize = 22;
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn halves_at(bytes: &[u8], at: usize) -> u32 {
    (u32::from(u16_at(bytes, at)) << 16) | u32::from(u16_at(bytes, at + 2))
}

fn put_halves(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 2].copy_from_slice(&((value >> 16) as u16).to_le_bytes());
    bytes[at + 2..at + 4].copy_from_slice(&(value as u16).to_le_bytes());
}

impl AddrPort {
    /// The address, or `None` for a tag v1 does not define.
    pub fn decode(&self) -> Option<SockAddr> {
        let p = &self.payload;
        Some(match self.tag {
            Addressfamily::INET4 => {
                let ip: [u8; 4] = p[inet::ADDR..inet::ADDR + 4].try_into().unwrap();
                SockAddr::Inet(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from(ip),
                    u16_at(p, inet::PORT),
                )))
            }
            Addressfamily::INET6 => {
                let ip: [u8; 16] = p[inet::ADDR..inet::ADDR + 16].try_into().unwrap();
                SockAddr::Inet(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from(ip),
                    u16_at(p, inet::PORT),
                    halves_at(p, inet::FLOW_INFO),
                    halves_at(p, inet::SCOPE_ID),
                )))
            }
            Addressfamily::UNIX => {
                let len = p.iter().position(|&b| b == 0).unwrap_or(p.len());
                SockAddr::Unix(p[..len].to_vec())
            }
            Addressfamily::UNSPEC => SockAddr::Unspec,
            _ => return None,
        })
    }

    /// `addr` as v1 reports it. A unix path keeps a NUL after it, as wasix-libc expects.
    pub fn encode(addr: &SockAddr) -> Self {
        let mut out = Self::ZERO;
        let p = &mut out.payload;
        match addr {
            SockAddr::Inet(SocketAddr::V4(v4)) => {
                out.tag = Addressfamily::INET4;
                p[inet::PORT..inet::PORT + 2].copy_from_slice(&v4.port().to_le_bytes());
                p[inet::ADDR..inet::ADDR + 4].copy_from_slice(&v4.ip().octets());
            }
            SockAddr::Inet(SocketAddr::V6(v6)) => {
                out.tag = Addressfamily::INET6;
                p[inet::PORT..inet::PORT + 2].copy_from_slice(&v6.port().to_le_bytes());
                p[inet::ADDR..inet::ADDR + 16].copy_from_slice(&v6.ip().octets());
                put_halves(p, inet::FLOW_INFO, v6.flowinfo());
                put_halves(p, inet::SCOPE_ID, v6.scope_id());
            }
            SockAddr::Unix(path) => {
                out.tag = Addressfamily::UNIX;
                let len = path.len().min(p.len() - 1);
                p[..len].copy_from_slice(&path[..len]);
            }
            SockAddr::Unspec => out.tag = Addressfamily::UNSPEC,
        }
        out
    }
}

impl AddrIp {
    pub fn new(ip: IpAddr) -> Self {
        let mut out = Self::ZERO;
        match ip {
            IpAddr::V4(v4) => {
                out.tag = Addressfamily::INET4;
                out.octets[..4].copy_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                out.tag = Addressfamily::INET6;
                out.octets = v6.octets();
            }
        }
        out
    }
}

impl AddrCidr {
    pub fn new(ip: IpAddr, prefix: u8) -> Self {
        let mut out = Self::ZERO;
        match ip {
            IpAddr::V4(v4) => {
                out.tag = Addressfamily::INET4;
                out.payload[..4].copy_from_slice(&v4.octets());
                out.payload[4] = prefix;
            }
            IpAddr::V6(v6) => {
                out.tag = Addressfamily::INET6;
                out.payload[..16].copy_from_slice(&v6.octets());
                out.payload[24] = prefix;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addr_port_round_trips() {
        let addrs = [
            SockAddr::Inet("127.0.0.1:8080".parse().unwrap()),
            SockAddr::Inet(SocketAddr::V6(SocketAddrV6::new(
                "fe80::1".parse().unwrap(),
                443,
                0x0001_0002,
                0x0003_0004,
            ))),
            SockAddr::Unix(b"/tmp/sock".to_vec()),
            SockAddr::Unspec,
        ];
        for addr in addrs {
            assert_eq!(AddrPort::encode(&addr).decode(), Some(addr));
        }
    }

    /// The bytes wasix-libc's `sockaddr_to_wasi` produces for `127.0.0.1:9000`.
    #[test]
    fn addr_port_matches_libc_bytes() {
        let mut bytes = [0u8; AddrPort::SIZE];
        bytes[0] = 1;
        bytes[2..4].copy_from_slice(&9000u16.to_le_bytes());
        bytes[4..8].copy_from_slice(&[127, 0, 0, 1]);
        let addr = AddrPort::read_from(&bytes).unwrap();
        assert_eq!(
            addr.decode(),
            Some(SockAddr::Inet("127.0.0.1:9000".parse().unwrap()))
        );
        let mut out = [0u8; AddrPort::SIZE];
        addr.write_to(&mut out);
        assert_eq!(out, bytes);
    }

    #[test]
    fn cidr_prefix_positions() {
        let v4 = AddrCidr::new("10.0.0.2".parse().unwrap(), 24);
        let mut bytes = [0u8; AddrCidr::SIZE];
        v4.write_to(&mut bytes);
        assert_eq!(&bytes[..7], &[1, 0, 10, 0, 0, 2, 24]);
        let v6 = AddrCidr::new("::1".parse().unwrap(), 128);
        v6.write_to(&mut bytes);
        assert_eq!((bytes[0], bytes[17], bytes[26]), (2, 1, 128));
    }
}
