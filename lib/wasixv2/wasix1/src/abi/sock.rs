//! Sockets and epoll (WASIX).

use super::{Fd, Ptr, layout, newtype_layout};

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

/// `epoll_ctl` (`u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpollCtl(pub u32);

impl EpollCtl {
    pub const ADD: Self = Self(0);
    pub const MOD: Self = Self(1);
    pub const DEL: Self = Self(2);
}

newtype_layout!(
    Addressfamily(u8),
    Socktype(u8),
    SockStatus(u8),
    RiFlags(u16),
    RoFlags(u16),
    SiFlags(u16),
    EpollType(u32),
    EpollCtl(u32),
);

layout! {
    /// `addr_port`: a tagged union of an address family and an address with a port. The
    /// payload starts at 2: `port: u16` (network order bytes) and the address for inet4 and
    /// inet6, the path bytes for unix.
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
    /// `addr_cidr`.
    pub struct AddrCidr (size 28, align 2) {
        pub tag: Addressfamily = 0,
        pub payload: [u8; 26] = 2,
    }
}

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
