//! ICMP: the kernel's own (`net/ipv4/icmp.c`, `net/ipv6/icmp.c`: echo replies, errors handed
//! to the protocol they quote), ping sockets (`SOCK_DGRAM` with `IPPROTO_ICMP` or
//! `IPPROTO_ICMPV6`, `net/ipv4/ping.c`, `net/ipv6/ping.c`) and raw ICMP sockets (`SOCK_RAW`,
//! `net/ipv4/raw.c`, `net/ipv6/raw.c`).
//!
//! There are still no packets: an ICMP message travels as what its IP packet would say
//! (addresses, hop limit, the ICMP bytes) from the namespace that sends it to the one that
//! holds its destination, inside a namespace or across a veth, or to a group's members. There
//! it is offered to every raw ICMP socket of its family first (`raw_v4_input`,
//! `ipv6_raw_deliver`), then handled: an echo request is answered back the way it came, an
//! echo reply goes to the ping socket of its identifier (`ping_rcv`), an error to the socket of
//! the datagram it quotes (`icmp_unreach`, `icmpv6_notify`). A UDP datagram to a port nobody
//! holds is answered with a port unreachable this way.
//!
//! Echo requests to the uplink go through the backend's ICMP endpoint (a host ICMP datagram
//! socket, see [`crate::Backend::ping`]); if the backend has none, the send fails with `EPERM`,
//! as a packet filter refusing it would make it.

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{Errno, IpOption, Ipv6Option, MsgFlags, SoEeOrigin, SocketLevel, SocketOption};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, BackendDatagram, Control, Domain, EchoIgnore, IcmpEntry, NetResult, QueuedError,
    RecvMsg, SendMsg, Shutdown, Socket, SocketKind,
    addr::{canonical_ip, present, unbound},
    bpf::{Arrival, PACKET_HOST, PACKET_LOOPBACK, PACKET_MULTICAST, SockFilter},
    cmsg::{self, SendCtl},
    iface::LOOPBACK_INDEX,
    net::{IpPath, Net},
    opts::{SockOpts, read_int, write_bytes, write_int},
    ports::SockId,
    ratelimit::{Cause, IcmpLimit, Limits, Outgoing},
    sync::MutexExt,
    udp::error_result,
};

pub(crate) const IPPROTO_ICMP: i32 = 1;
pub(crate) const IPPROTO_ICMPV6: i32 = 58;
pub(crate) const IPPROTO_UDP: u8 = 17;

const ICMP_ECHOREPLY: u8 = 0;
const ICMP_DEST_UNREACH: u8 = 3;
const ICMP_ECHO: u8 = 8;
const ICMP_TIME_EXCEEDED: u8 = 11;
const ICMP_PARAMETERPROB: u8 = 12;
pub(crate) const ICMP_PORT_UNREACH: u8 = 3;
const ICMP_HOST_UNREACH: u8 = 1;
const ICMPV6_DEST_UNREACH: u8 = 1;
const ICMPV6_PKT_TOOBIG: u8 = 2;
const ICMPV6_TIME_EXCEED: u8 = 3;
const ICMPV6_PARAMPROB: u8 = 4;
pub(crate) const ICMPV6_PORT_UNREACH: u8 = 4;
const ICMPV6_ADDR_UNREACH: u8 = 3;
const ICMPV6_ECHO_REQUEST: u8 = 128;
const ICMPV6_ECHO_REPLY: u8 = 129;

/// `SOL_RAW`'s `ICMP_FILTER`, `SOL_ICMPV6`'s `ICMP6_FILTER`, `IPV6_CHECKSUM`.
const SOL_RAW: i32 = SocketLevel::SOL_RAW.0;
const SOL_ICMPV6: i32 = SocketLevel::SOL_ICMPV6.0;
const SOL_IP: i32 = SocketLevel::SOL_IP.0;
const SOL_IPV6: i32 = SocketLevel::SOL_IPV6.0;
const SOL_SOCKET: i32 = SocketLevel::SOL_SOCKET.0;
const ICMP_FILTER: i32 = 1;
const ICMP6_FILTER: i32 = 1;
const IPV6_CHECKSUM: i32 = Ipv6Option::IPV6_CHECKSUM.0;

/// `sysctl_ip_default_ttl` and IPv6's `hop_limit`: what a unicast packet leaves with.
const DEFAULT_TTL: u8 = 64;
/// What a queued message is charged against `SO_RCVBUF` on top of its bytes (as UDP's).
const OVERHEAD: usize = 768;
/// The ICMP header: type, code, checksum, and the identifier and sequence of an echo.
const ICMP_HDR: usize = 8;
pub(crate) const IPV4_HDR: usize = 20;
pub(crate) const IPV6_HDR: usize = 40;
pub(crate) const UDP_HDR: usize = 8;
/// How much of the offending packet an ICMP error quotes (`icmp_send`: 576 bytes of error
/// packet; `icmp6_send`: the IPv6 minimum MTU).
const ICMP_ERR_ROOM: usize = 576 - IPV4_HDR - ICMP_HDR;
const ICMP6_ERR_ROOM: usize = 1280 - IPV6_HDR - ICMP_HDR;

/// An ICMP message on its way: what its IP packet would carry.
#[derive(Clone, Debug)]
pub(crate) struct IcmpMsg {
    pub src: IpAddr,
    pub dst: IpAddr,
    /// The IPv4 TTL or IPv6 hop limit it arrives with.
    pub ttl: u8,
    /// The ICMP message: header and body.
    pub data: Vec<u8>,
}

/// The namespace a message came from and the interface it comes back in on there, which an
/// answer takes.
#[derive(Clone)]
pub(crate) struct Back {
    pub net: Arc<Net>,
    pub iif: u32,
}

/// The ICMP state of a namespace: its ping sockets by identifier (`ping_table`), its raw ICMP
/// sockets, and the sysctls the kernel's ICMP heeds, rate limits among them.
#[derive(Debug)]
pub(crate) struct IcmpTables {
    /// (identifier, socket, `SO_REUSEADDR`, socket), oldest first.
    ping: Vec<(u16, SockId, bool, Weak<IcmpSocket>)>,
    /// `ping_port_rover`: the last identifier given out.
    rover: u16,
    pub(crate) raw: Vec<Weak<IcmpSocket>>,
    /// `net.ipv4.icmp_echo_ignore_all`, `net.ipv4.icmp_echo_ignore_broadcasts`.
    pub echo_ignore_all: bool,
    pub echo_ignore_broadcasts: bool,
    /// `net.ipv6.icmp.echo_ignore_all`, `net.ipv6.icmp.echo_ignore_multicast`.
    pub echo_ignore_all6: bool,
    pub echo_ignore_multicast6: bool,
    /// The identification of the next IPv4 header made up for a raw socket.
    ip_id: u16,
    pub limits: Limits,
}

impl Default for IcmpTables {
    fn default() -> Self {
        Self {
            ping: Vec::new(),
            rover: 0,
            raw: Vec::new(),
            echo_ignore_all: false,
            echo_ignore_broadcasts: true,
            echo_ignore_all6: false,
            echo_ignore_multicast6: false,
            ip_id: 1,
            limits: Limits::default(),
        }
    }
}

impl IcmpTables {
    /// The bound ping sockets and the raw sockets, newest first.
    fn sockets(&self) -> Vec<Weak<IcmpSocket>> {
        let pings = self.ping.iter().rev().map(|e| e.3.clone());
        pings.chain(self.raw.iter().rev().cloned()).collect()
    }

    /// `ping_get_port`: `ident`, unless a socket holds it and not both set `SO_REUSEADDR`;
    /// 0 takes the next free identifier after the last given out, never 0. `EADDRINUSE`
    /// when taken or none is free.
    fn bind(
        &mut self,
        id: SockId,
        ident: u16,
        reuse: bool,
        sock: Weak<IcmpSocket>,
    ) -> NetResult<u16> {
        self.ping.retain(|e| e.3.strong_count() > 0);
        let ident = match ident {
            0 => {
                let mut candidate = self.rover;
                let free = (0..u16::MAX).find_map(|_| {
                    candidate = candidate.wrapping_add(1).max(1);
                    (!self.ping.iter().any(|e| e.0 == candidate)).then_some(candidate)
                });
                let ident = free.ok_or(Errno::EADDRINUSE)?;
                self.rover = ident;
                ident
            }
            ident => {
                let taken = self
                    .ping
                    .iter()
                    .any(|e| e.0 == ident && e.1 != id && !(e.2 && reuse));
                if taken {
                    return Err(Errno::EADDRINUSE);
                }
                ident
            }
        };
        self.ping.push((ident, id, reuse, sock));
        Ok(ident)
    }

    fn unbind(&mut self, id: SockId) {
        self.ping.retain(|e| e.1 != id);
    }

    /// `ping_lookup`'s candidates for `ident`, newest first.
    fn pings(&self, ident: u16) -> Vec<Weak<IcmpSocket>> {
        self.ping
            .iter()
            .rev()
            .filter(|e| e.0 == ident)
            .map(|e| e.3.clone())
            .collect()
    }

    fn next_ip_id(&mut self) -> u16 {
        let id = self.ip_id;
        self.ip_id = self.ip_id.wrapping_add(1);
        id
    }
}

// ---- checksums and headers ----

fn sum16(data: &[u8]) -> u32 {
    data.chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum()
}

fn fold(mut sum: u32) -> u16 {
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// The pseudo header's sum for an upper-layer checksum (RFC 768, RFC 8200 8.1).
fn pseudo_sum(src: IpAddr, dst: IpAddr, proto: u8, len: usize) -> u32 {
    match (src, dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => {
            sum16(&s.octets()) + sum16(&d.octets()) + u32::from(proto) + len as u32
        }
        _ => {
            let v6 = |ip: IpAddr| match ip {
                IpAddr::V6(v6) => v6.octets(),
                IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
            };
            sum16(&v6(src))
                + sum16(&v6(dst))
                + sum16(&(len as u32).to_be_bytes())
                + u32::from(proto)
        }
    }
}

/// The checksum an ICMP message carries: over the message for ICMP, over the pseudo header
/// too for ICMPv6.
fn icmp_checksum(src: IpAddr, dst: IpAddr, data: &[u8]) -> u16 {
    match dst {
        IpAddr::V4(_) => fold(sum16(data)),
        IpAddr::V6(_) => fold(pseudo_sum(src, dst, IPPROTO_ICMPV6 as u8, data.len()) + sum16(data)),
    }
}

/// Writes the upper-layer checksum of a packet of `proto` at `offset`, over the pseudo header
/// (`csum_ipv6_magic`), as a raw IPv6 socket with `IPV6_CHECKSUM` has it.
pub(crate) fn set_transport_checksum(
    src: IpAddr,
    dst: IpAddr,
    proto: u8,
    data: &mut [u8],
    offset: usize,
) {
    data[offset..offset + 2].fill(0);
    let csum = fold(pseudo_sum(src, dst, proto, data.len()) + sum16(data));
    data[offset..offset + 2].copy_from_slice(&csum.to_be_bytes());
}

/// Whether a UDP datagram's checksum holds; 0 is none (IPv4 only, RFC 768).
pub(crate) fn udp_checksum_ok(src: IpAddr, dst: IpAddr, udp: &[u8]) -> bool {
    let stored = u16::from_be_bytes([udp[6], udp[7]]);
    if stored == 0 {
        return src.is_ipv4();
    }
    fold(pseudo_sum(src, dst, IPPROTO_UDP, udp.len()) + sum16(udp)) == 0
}

/// Writes the message's checksum (`csum_ipv6_magic`, `ip_compute_csum`) at `offset`.
pub(crate) fn set_checksum(src: IpAddr, dst: IpAddr, data: &mut [u8], offset: usize) {
    data[offset..offset + 2].fill(0);
    let csum = icmp_checksum(src, dst, data);
    data[offset..offset + 2].copy_from_slice(&csum.to_be_bytes());
}

fn checksum_ok(msg: &IcmpMsg) -> bool {
    let sum = match msg.dst {
        IpAddr::V4(_) => sum16(&msg.data),
        IpAddr::V6(_) => {
            pseudo_sum(msg.src, msg.dst, IPPROTO_ICMPV6 as u8, msg.data.len()) + sum16(&msg.data)
        }
    };
    fold(sum) == 0
}

/// An IPv4 header for `len` bytes of protocol `proto`, as a raw IPv4 socket receives it.
pub(crate) fn ipv4_header(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    proto: u8,
    ttl: u8,
    len: usize,
    id: u16,
) -> [u8; IPV4_HDR] {
    let mut h = [0u8; IPV4_HDR];
    h[0] = 0x45;
    h[2..4].copy_from_slice(&((IPV4_HDR + len) as u16).to_be_bytes());
    h[4..6].copy_from_slice(&id.to_be_bytes());
    h[8] = ttl;
    h[9] = proto;
    h[12..16].copy_from_slice(&src.octets());
    h[16..20].copy_from_slice(&dst.octets());
    let csum = fold(sum16(&h));
    h[10..12].copy_from_slice(&csum.to_be_bytes());
    h
}

pub(crate) fn ipv6_header(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    proto: u8,
    hops: u8,
    len: usize,
) -> [u8; IPV6_HDR] {
    let mut h = [0u8; IPV6_HDR];
    h[0] = 0x60;
    h[4..6].copy_from_slice(&(len as u16).to_be_bytes());
    h[6] = proto;
    h[7] = hops;
    h[8..24].copy_from_slice(&src.octets());
    h[24..40].copy_from_slice(&dst.octets());
    h
}

fn v4(ip: IpAddr) -> Ipv4Addr {
    match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(v6) => v6.to_ipv4_mapped().unwrap_or(Ipv4Addr::UNSPECIFIED),
    }
}

pub(crate) fn v6(ip: IpAddr) -> Ipv6Addr {
    match ip {
        IpAddr::V6(v6) => v6,
        IpAddr::V4(v4) => v4.to_ipv6_mapped(),
    }
}

/// A UDP datagram from `src` to `dst`: its header, with the checksum, then `payload`.
pub(crate) fn udp_datagram(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let udp_len = UDP_HDR + payload.len();
    let mut udp = Vec::with_capacity(udp_len);
    udp.extend(src.port().to_be_bytes());
    udp.extend(dst.port().to_be_bytes());
    udp.extend((udp_len as u16).to_be_bytes());
    udp.extend([0, 0]);
    udp.extend_from_slice(payload);
    let csum = fold(pseudo_sum(src.ip(), dst.ip(), IPPROTO_UDP, udp_len) + sum16(&udp));
    udp[6..8].copy_from_slice(&csum.max(1).to_be_bytes());
    udp
}

/// The ICMP error `type_`/`code` sent from `from` to the sender of a UDP datagram from
/// `src` to `dst` with `payload`, quoting as much of it as fits (`icmp_send`, `icmp6_send`).
pub(crate) fn udp_error(
    type_: u8,
    code: u8,
    from: IpAddr,
    src: SocketAddr,
    dst: SocketAddr,
    payload: &[u8],
) -> IcmpMsg {
    let udp = udp_datagram(src, dst, payload);
    quote_error(type_, code, from, src.ip(), dst.ip(), IPPROTO_UDP, &udp)
}

/// The ICMP error `type_`/`code` sent from `from` about a packet of protocol `proto` from
/// `src` to `dst` carrying `transport`, quoting its IP header and as much of the rest as
/// fits.
pub(crate) fn quote_error(
    type_: u8,
    code: u8,
    from: IpAddr,
    src: IpAddr,
    dst: IpAddr,
    proto: u8,
    transport: &[u8],
) -> IcmpMsg {
    let len = transport.len();
    let (header, room): (Vec<u8>, usize) = match dst {
        IpAddr::V4(_) => (
            ipv4_header(v4(src), v4(dst), proto, DEFAULT_TTL, len, 0).to_vec(),
            ICMP_ERR_ROOM,
        ),
        IpAddr::V6(_) => (
            ipv6_header(v6(src), v6(dst), proto, DEFAULT_TTL, len).to_vec(),
            ICMP6_ERR_ROOM,
        ),
    };
    let mut data = vec![type_, code, 0, 0, 0, 0, 0, 0];
    data.extend(header.iter().chain(transport).take(room));
    set_checksum(from, src, &mut data, 2);
    IcmpMsg {
        src: from,
        dst: src,
        ttl: DEFAULT_TTL,
        data,
    }
}

/// `icmp_err_convert` and `icmpv6_err_convert`: the errno an ICMP error stands for, and
/// whether it is hard (reported to a connected socket without `IP_RECVERR`). `None` for
/// messages that are no error of a socket's.
fn err_convert(v6: bool, type_: u8, code: u8) -> Option<(Errno, bool)> {
    use Errno as E;
    if !v6 {
        return match type_ {
            ICMP_DEST_UNREACH => Some(match code {
                0 => (E::ENETUNREACH, false),
                1 => (E::EHOSTUNREACH, false),
                2 => (E::ENOPROTOOPT, true),
                3 => (E::ECONNREFUSED, true),
                4 => (E::EMSGSIZE, false),
                5 => (E::EOPNOTSUPP, false),
                6 => (E::ENETUNREACH, true),
                7 => (E::EHOSTDOWN, true),
                8 => (E::ENONET, true),
                9 => (E::ENETUNREACH, true),
                10..=12 => (E::EHOSTUNREACH, code == 10),
                13..=15 => (E::EHOSTUNREACH, true),
                _ => (E::EHOSTUNREACH, false),
            }),
            ICMP_TIME_EXCEEDED => Some((E::EHOSTUNREACH, false)),
            ICMP_PARAMETERPROB => Some((E::EPROTO, true)),
            _ => None,
        };
    }
    match type_ {
        ICMPV6_DEST_UNREACH => Some(match code {
            0 => (E::ENETUNREACH, false),
            1 => (E::EACCES, true),
            2 | 3 => (E::EHOSTUNREACH, false),
            4 => (E::ECONNREFUSED, true),
            5 | 6 => (E::EACCES, true),
            _ => (E::EPROTO, false),
        }),
        ICMPV6_PKT_TOOBIG => Some((E::EMSGSIZE, true)),
        ICMPV6_PARAMPROB => Some((E::EPROTO, true)),
        ICMPV6_TIME_EXCEED => Some((E::EHOSTUNREACH, false)),
        _ => None,
    }
}

/// What an ICMP error says about the packet it quotes.
pub(crate) struct Quoted<'a> {
    pub errno: Errno,
    pub hard: bool,
    pub type_: u8,
    pub code: u8,
    pub info: u32,
    /// The node that sent the error.
    pub offender: IpAddr,
    /// The quoted packet's addresses and protocol.
    pub src: IpAddr,
    pub dst: IpAddr,
    pub proto: u8,
    /// What follows the quoted IP header.
    pub transport: &'a [u8],
}

fn parse_error(msg: &IcmpMsg) -> Option<Quoted<'_>> {
    let v6 = msg.dst.is_ipv6();
    let (type_, code) = (msg.data[0], msg.data[1]);
    let (errno, hard) = err_convert(v6, type_, code)?;
    let info = u32::from_be_bytes(msg.data[4..8].try_into().expect("4 bytes"));
    let ip = &msg.data[ICMP_HDR..];
    let (src, dst, proto, transport) = match v6 {
        false => {
            let ihl = usize::from(*ip.first()? & 0x0f) * 4;
            if ip.len() < ihl.max(IPV4_HDR) || ip[0] >> 4 != 4 {
                return None;
            }
            let at = |i: usize| IpAddr::V4(Ipv4Addr::new(ip[i], ip[i + 1], ip[i + 2], ip[i + 3]));
            (at(12), at(16), ip[9], &ip[ihl..])
        }
        true => {
            if ip.len() < IPV6_HDR || ip[0] >> 4 != 6 {
                return None;
            }
            let at = |i: usize| {
                IpAddr::V6(Ipv6Addr::from(
                    <[u8; 16]>::try_from(&ip[i..i + 16]).expect("16"),
                ))
            };
            (at(8), at(24), ip[6], &ip[IPV6_HDR..])
        }
    };
    Some(Quoted {
        errno,
        hard,
        type_,
        code,
        info,
        offender: msg.src,
        src,
        dst,
        proto,
        transport,
    })
}

// ---- the kernel's ICMP ----

impl Net {
    /// An ICMP message arrived on interface `iif` from the namespace `back` names
    /// (`icmp_rcv`, `icmpv6_rcv`), at `now` on the kernel's monotonic clock.
    pub(crate) fn icmp_input(&self, msg: IcmpMsg, iif: u32, back: Back, now: Duration) {
        let pkttype = self.pkttype(msg.dst, &back);
        let proto = match msg.dst {
            IpAddr::V4(_) => IPPROTO_ICMP,
            IpAddr::V6(_) => IPPROTO_ICMPV6,
        } as u8;
        if self.has_raw(proto) {
            let packet = crate::raw::Packet {
                src: msg.src,
                dst: msg.dst,
                ttl: msg.ttl,
                proto,
                data: msg.data.clone(),
            };
            self.raw_input(&packet, iif, pkttype);
        }
        if msg.data.len() < ICMP_HDR || !checksum_ok(&msg) {
            return;
        }
        let v6 = msg.dst.is_ipv6();
        match (v6, msg.data[0]) {
            (false, ICMP_ECHO) | (true, ICMPV6_ECHO_REQUEST) => {
                self.icmp_echo(&msg, iif, back, now)
            }
            // `icmp_rcv` drops everything but echoes sent to a group.
            _ if !v6 && msg.dst.is_multicast() => {}
            (false, ICMP_ECHOREPLY) | (true, ICMPV6_ECHO_REPLY) => self.ping_input(&msg, iif),
            _ => {
                if let Some(q) = parse_error(&msg) {
                    self.icmp_error(&q);
                }
            }
        }
    }

    /// `icmp_echo`, `icmpv6_echo_reply`: answers an echo request back the way it came, from
    /// the address it was sent to, or for a group from an address of the interface it
    /// arrived on. IPv4 heeds `icmp_echo_ignore_all` and, for a group,
    /// `icmp_echo_ignore_broadcasts`; IPv6 `echo_ignore_all` and, for a group,
    /// `echo_ignore_multicast` (`icmpv6_rcv`). Then the rate limits apply (`icmp_reply`,
    /// `icmpv6_echo_reply`).
    fn icmp_echo(&self, msg: &IcmpMsg, iif: u32, back: Back, now: Duration) {
        let type_ = match msg.dst {
            IpAddr::V4(_) => ICMP_ECHOREPLY,
            IpAddr::V6(_) => ICMPV6_ECHO_REPLY,
        };
        let src = {
            let mut st = self.state();
            let group = msg.dst.is_multicast();
            let ignored = match msg.dst {
                IpAddr::V4(_) => {
                    st.icmp.echo_ignore_all || (group && st.icmp.echo_ignore_broadcasts)
                }
                IpAddr::V6(_) => {
                    st.icmp.echo_ignore_all6 || (group && st.icmp.echo_ignore_multicast6)
                }
            };
            let reply = Outgoing {
                v6: msg.dst.is_ipv6(),
                type_,
                code: 0,
                cause: Cause::Echo,
                dst: msg.src,
                in_loopback: iif == LOOPBACK_INDEX,
                out_loopback: self.is(&back.net),
            };
            if ignored || !st.icmp.limits.allow(&reply, now) {
                return;
            }
            match msg.dst.is_multicast() {
                false => Some(msg.dst),
                true => st.ifaces.source_on(iif, msg.src),
            }
        };
        let Some(src) = src else {
            return;
        };
        let mut data = msg.data.clone();
        data[0] = type_;
        set_checksum(src, msg.src, &mut data, 2);
        let reply = IcmpMsg {
            src,
            dst: msg.src,
            ttl: DEFAULT_TTL,
            data,
        };
        let me = Back {
            net: self.arc(),
            iif,
        };
        back.net.icmp_input(reply, back.iif, me, now);
    }

    /// What a packet to `dst` arrives as: a group's is the sender's own looped-back copy, or
    /// one received from another namespace.
    pub(crate) fn pkttype(&self, dst: IpAddr, back: &Back) -> u8 {
        match dst.is_multicast() {
            false => PACKET_HOST,
            true if self.is(&back.net) => PACKET_LOOPBACK,
            true => PACKET_MULTICAST,
        }
    }

    /// Whether `net` is this namespace, where a message goes through loopback.
    pub(crate) fn is(&self, net: &Arc<Net>) -> bool {
        std::ptr::eq(Arc::as_ptr(net), self)
    }

    /// Whether the rate limits let an error about a packet go (`__icmp_send`, `icmp6_send`).
    pub(crate) fn error_allowed(
        &self,
        msg: &IcmpMsg,
        in_loopback: bool,
        out_loopback: bool,
        now: Duration,
    ) -> bool {
        let out = Outgoing {
            v6: msg.dst.is_ipv6(),
            type_: msg.data[0],
            code: msg.data[1],
            cause: Cause::Error,
            dst: msg.dst,
            in_loopback,
            out_loopback,
        };
        self.state().icmp.limits.allow(&out, now)
    }

    /// `ping_rcv`: an echo reply goes to the ping socket its identifier names.
    fn ping_input(&self, msg: &IcmpMsg, iif: u32) {
        let ident = u16::from_be_bytes([msg.data[4], msg.data[5]]);
        let candidates = self.state().icmp.pings(ident);
        for sock in candidates.iter().filter_map(Weak::upgrade) {
            if sock.ping_match(msg.dst, iif) {
                sock.queue(msg.data.clone(), msg, iif, PACKET_HOST);
                return;
            }
        }
    }

    /// `icmp_unreach` and `icmpv6_notify`: an error goes to the protocol of the packet it
    /// quotes: a UDP socket (`__udp4_lib_err`), or the ping socket of a quoted echo request
    /// (`ping_err`).
    fn icmp_error(&self, q: &Quoted<'_>) {
        if q.transport.len() < 8 {
            return;
        }
        let port = |at: usize| u16::from_be_bytes([q.transport[at], q.transport[at + 1]]);
        match q.proto {
            IPPROTO_UDP => {
                let (src, dst) = (
                    SocketAddr::new(q.src, port(0)),
                    SocketAddr::new(q.dst, port(2)),
                );
                let sock = {
                    let st = self.state();
                    let live = |w: &Weak<crate::UdpSocket>, _| w.strong_count() > 0;
                    st.udp.lookup_one(src, Some(dst), live, dst.port() as usize)
                };
                if let Some(sock) = sock.and_then(|w| w.upgrade()) {
                    sock.icmp_error(q, dst, &q.transport[UDP_HDR..]);
                }
            }
            p if i32::from(p) == IPPROTO_ICMP || i32::from(p) == IPPROTO_ICMPV6 => {
                let echo = match q.dst {
                    IpAddr::V4(_) => ICMP_ECHO,
                    IpAddr::V6(_) => ICMPV6_ECHO_REQUEST,
                };
                if q.transport[0] != echo {
                    return;
                }
                let candidates = self.state().icmp.pings(port(4));
                for sock in candidates.iter().filter_map(Weak::upgrade) {
                    if sock.ping_match(q.src, 0) {
                        sock.ping_error(q);
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    /// Sends an ICMP message along `path` (the route a socket looked up) at `now`.
    fn icmp_deliver(&self, msg: IcmpMsg, path: IpPath, now: Duration) {
        match path {
            IpPath::Local {
                net, iif, back_iif, ..
            } => {
                let back = Back {
                    net: self.arc(),
                    iif: back_iif,
                };
                net.icmp_input(msg, iif, back, now);
            }
            IpPath::Nowhere {
                unresolved: true, ..
            } => {
                let proto = match msg.dst {
                    IpAddr::V4(_) => IPPROTO_ICMP,
                    IpAddr::V6(_) => IPPROTO_ICMPV6,
                } as u8;
                self.neighbour_failed(msg.src, msg.dst, proto, &msg.data, now);
            }
            _ => {}
        }
    }

    /// A UDP datagram from `src` to `dst` that arrived on `iif` and no socket of this
    /// namespace takes: the port unreachable `__udp4_lib_rcv` and `__udp6_lib_rcv` send goes
    /// back to the sender's namespace, as the rate limits allow at `now`.
    pub(crate) fn udp_unreachable(
        &self,
        src: SocketAddr,
        dst: SocketAddr,
        payload: &[u8],
        iif: u32,
        back: Back,
        now: Duration,
    ) {
        let (type_, code) = match dst {
            SocketAddr::V4(_) => (ICMP_DEST_UNREACH, ICMP_PORT_UNREACH),
            SocketAddr::V6(_) => (ICMPV6_DEST_UNREACH, ICMPV6_PORT_UNREACH),
        };
        let msg = udp_error(type_, code, dst.ip(), src, dst, payload);
        if !self.error_allowed(&msg, iif == LOOPBACK_INDEX, self.is(&back.net), now) {
            return;
        }
        let me = Back {
            net: self.arc(),
            iif: 0,
        };
        back.net.icmp_input(msg, back.iif, me, now);
    }

    /// An ICMP message to the namespace's DNS server, a host on the uplink: it answers echo
    /// requests, as slirp's does, and takes nothing else.
    pub(crate) fn dns_icmp(&self, msg: IcmpMsg, now: Duration) {
        let (request, reply) = match msg.dst {
            IpAddr::V4(_) => (ICMP_ECHO, ICMP_ECHOREPLY),
            IpAddr::V6(_) => (ICMPV6_ECHO_REQUEST, ICMPV6_ECHO_REPLY),
        };
        if msg.data.len() < ICMP_HDR || msg.data[0] != request || !checksum_ok(&msg) {
            return;
        }
        let mut data = msg.data;
        data[0] = reply;
        set_checksum(msg.dst, msg.src, &mut data, 2);
        let reply = IcmpMsg {
            src: msg.dst,
            dst: msg.src,
            ttl: DEFAULT_TTL,
            data,
        };
        self.deliver_from_dns_server(reply, now);
    }

    /// A UDP datagram from `src` to a port of the DNS server other than 53: the server's port
    /// unreachable.
    pub(crate) fn dns_port_unreachable(
        &self,
        src: SocketAddr,
        dst: SocketAddr,
        payload: &[u8],
        now: Duration,
    ) {
        let (type_, code) = match dst {
            SocketAddr::V4(_) => (ICMP_DEST_UNREACH, ICMP_PORT_UNREACH),
            SocketAddr::V6(_) => (ICMPV6_DEST_UNREACH, ICMPV6_PORT_UNREACH),
        };
        let msg = udp_error(type_, code, dst.ip(), src, dst, payload);
        self.deliver_from_dns_server(msg, now);
    }

    /// An ICMP message from the DNS server, which arrives on the uplink.
    fn deliver_from_dns_server(&self, msg: IcmpMsg, now: Duration) {
        let iif = self.uplink_index();
        let back = Back {
            net: self.arc(),
            iif,
        };
        self.icmp_input(msg, iif, back, now);
    }

    /// The namespace's ping and raw ICMP sockets ([`crate::Network::icmp_sockets`]); read
    /// once the namespace's lock is gone, as sockets lock themselves first.
    pub(crate) fn icmp_entries(&self) -> Vec<IcmpEntry> {
        let socks = self.state().icmp.sockets();
        let socks: Vec<_> = socks.iter().filter_map(Weak::upgrade).collect();
        socks.iter().map(|s| s.entry()).collect()
    }

    /// `arp_error_report` and `ip6_link_failure`: a packet of protocol `proto` from `src`
    /// to `dst` whose next hop no neighbour answers for comes back from the sender's own
    /// address as a host (address) unreachable quoting it, through loopback. Linux sends it
    /// once resolution gives up; here it comes at once. Never about an ICMP error
    /// (`icmp_pointers[].error`, `is_ineligible`) or to a group. The packet was on its way
    /// out another device, so the global rate limit applies at `now`, the destination's not
    /// (the message goes to this namespace, through loopback).
    pub(crate) fn neighbour_failed(
        &self,
        src: IpAddr,
        dst: IpAddr,
        proto: u8,
        transport: &[u8],
        now: Duration,
    ) {
        let v6 = dst.is_ipv6();
        let icmp_error = match (v6, transport.first()) {
            (false, Some(t)) if i32::from(proto) == IPPROTO_ICMP => {
                matches!(
                    *t,
                    ICMP_DEST_UNREACH | 4 | 5 | ICMP_TIME_EXCEEDED | ICMP_PARAMETERPROB
                )
            }
            (true, Some(t)) if i32::from(proto) == IPPROTO_ICMPV6 => *t < 128,
            _ => false,
        };
        if icmp_error || dst.is_multicast() || src.is_unspecified() {
            return;
        }
        let (type_, code) = match v6 {
            false => (ICMP_DEST_UNREACH, ICMP_HOST_UNREACH),
            true => (ICMPV6_DEST_UNREACH, ICMPV6_ADDR_UNREACH),
        };
        let msg = quote_error(type_, code, src, src, dst, proto, transport);
        if !self.error_allowed(&msg, false, true, now) {
            return;
        }
        let back = Back {
            net: self.arc(),
            iif: LOOPBACK_INDEX,
        };
        self.icmp_input(msg, LOOPBACK_INDEX, back, now);
    }

    /// One of the rate-limit sysctls ([`IcmpLimit`]).
    pub(crate) fn set_limit(&self, limit: IcmpLimit) {
        self.state().icmp.limits.set(limit);
    }

    /// One of the echo sysctls ([`EchoIgnore`]).
    pub(crate) fn set_echo_ignore(&self, which: EchoIgnore, ignore: bool) {
        let mut st = self.state();
        let icmp = &mut st.icmp;
        *match which {
            EchoIgnore::All => &mut icmp.echo_ignore_all,
            EchoIgnore::Broadcasts => &mut icmp.echo_ignore_broadcasts,
            EchoIgnore::All6 => &mut icmp.echo_ignore_all6,
            EchoIgnore::Multicast6 => &mut icmp.echo_ignore_multicast6,
        } = ignore;
    }
}

// ---- sockets ----

/// One received message: the ICMP message (with an IPv4 header for a raw IPv4 socket) and
/// what its packet said.
struct Received {
    data: Vec<u8>,
    from: IpAddr,
    dst: IpAddr,
    iif: u32,
    ttl: u8,
}

struct Inner {
    opts: SockOpts,
    /// `inet_rcv_saddr`: the bound address; `None` is the wildcard.
    local: Option<IpAddr>,
    /// A ping socket's identifier (`inet_num`), once bound.
    ident: Option<u16>,
    /// `SOCK_BINDADDR_LOCK`.
    addr_locked: bool,
    peer: Option<IpAddr>,
    /// `inet_dport`: the port a ping socket connected with.
    dport: u16,
    queue: VecDeque<Received>,
    queued: usize,
    /// `sk_drops`.
    drops: u32,
    err: Option<Errno>,
    errqueue: VecDeque<QueuedError>,
    /// `ICMP_FILTER` (the first word) and `ICMP6_FILTER`: types not to deliver.
    filter: [u32; 8],
    /// `IPV6_CHECKSUM` of a raw IPv6 socket: where the kernel puts the checksum, `None` for
    /// nowhere.
    csum_offset: Option<usize>,
    /// `IP_HDRINCL`.
    hdrincl: bool,
    rd_shut: bool,
    wr_shut: bool,
    /// The backend's ICMP endpoint, while the socket has one; the source address it sends
    /// from, and the identifier of the last echo request sent through it, which the host
    /// replaces with its own.
    backend: Option<(Arc<dyn BackendDatagram>, IpAddr)>,
    echo_id: u16,
}

/// A ping socket (`SOCK_DGRAM`) or a raw socket (`SOCK_RAW`, of ICMP or any other protocol,
/// see [`crate::raw`]) of a [`Net`].
pub struct IcmpSocket {
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    raw: bool,
    /// The protocol a raw socket takes and sends (`inet_num`); ICMP's for a ping socket.
    proto: u8,
    /// `sk_uid`.
    uid: u32,
    ino: AtomicU64,
    wq: WaitQueue,
    me: Weak<IcmpSocket>,
    inner: Mutex<Inner>,
}

impl IcmpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain, raw: bool, uid: u32) -> Arc<Self> {
        let icmp = match domain {
            Domain::Inet => IPPROTO_ICMP,
            Domain::Inet6 => IPPROTO_ICMPV6,
        } as u8;
        Self::with_protocol(net, domain, raw, icmp, uid)
    }

    /// A raw socket of `proto` (`inet_create`, `inet6_create`): `IPPROTO_RAW` sends with
    /// `IP_HDRINCL`; on IPv6, ICMPv6 and Mobility Header sockets have their checksum computed
    /// (`rawv6_init_sk`).
    pub(crate) fn raw(net: Arc<Net>, domain: Domain, proto: u8, uid: u32) -> Arc<Self> {
        Self::with_protocol(net, domain, true, proto, uid)
    }

    fn with_protocol(net: Arc<Net>, domain: Domain, raw: bool, proto: u8, uid: u32) -> Arc<Self> {
        let id = net.next_id();
        let kind = match raw {
            true => SocketKind::Raw,
            false => SocketKind::Datagram,
        };
        let sock = Arc::new_cyclic(|me| Self {
            net: net.clone(),
            id,
            domain,
            raw,
            proto,
            uid,
            ino: AtomicU64::new(0),
            wq: WaitQueue::new(),
            me: me.clone(),
            inner: Mutex::new(Inner {
                opts: SockOpts::new(kind, domain),
                local: None,
                ident: None,
                addr_locked: false,
                peer: None,
                dport: 0,
                queue: VecDeque::new(),
                queued: 0,
                drops: 0,
                err: None,
                errqueue: VecDeque::new(),
                filter: [0; 8],
                csum_offset: match (raw, domain, proto) {
                    (true, Domain::Inet6, p) if i32::from(p) == IPPROTO_ICMPV6 => Some(2),
                    (true, Domain::Inet6, crate::raw::IPPROTO_MH) => Some(4),
                    _ => None,
                },
                hdrincl: raw && proto == crate::raw::IPPROTO_RAW,
                rd_shut: false,
                wr_shut: false,
                backend: None,
                echo_id: 0,
            }),
        });
        if raw {
            net.state().icmp.raw.push(Arc::downgrade(&sock));
        }
        sock
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_or_recover()
    }

    /// The socket as `ping_v4_format_sock` and `raw_sock_seq_show` read it.
    fn entry(&self) -> IcmpEntry {
        let g = self.lock();
        IcmpEntry {
            domain: self.domain,
            raw: self.raw,
            local: g.local,
            num: match self.raw {
                true => self.protocol() as u16,
                false => g.ident.unwrap_or(0),
            },
            peer: g.peer.map(|p| (p, g.dport)),
            rx_queue: g.queued,
            uid: self.uid,
            inode: self.ino.load(Ordering::Relaxed),
            drops: g.drops,
        }
    }

    fn v6(&self) -> bool {
        self.domain == Domain::Inet6
    }

    fn any(&self) -> IpAddr {
        match self.domain {
            Domain::Inet => Ipv4Addr::UNSPECIFIED.into(),
            Domain::Inet6 => Ipv6Addr::UNSPECIFIED.into(),
        }
    }

    /// `inet_autobind` with `ping_get_port`: a ping socket takes an identifier before it
    /// sends or connects.
    fn autobind(&self, g: &mut Inner) -> NetResult<u16> {
        if let Some(ident) = g.ident {
            return Ok(ident);
        }
        let ident = self
            .net
            .state()
            .icmp
            .bind(self.id, 0, g.opts.reuseaddr, self.me.clone())?;
        g.ident = Some(ident);
        Ok(ident)
    }

    fn device(&self, g: &Inner) -> u32 {
        self.net.device_index(&g.opts.bind_device)
    }

    /// `ping_lookup`'s test: bound to the message's destination or the wildcard, and on its
    /// interface if bound to a device.
    fn ping_match(&self, dst: IpAddr, iif: u32) -> bool {
        let g = self.lock();
        let dev = self.device(&g);
        dst.is_ipv6() == self.v6()
            && g.local.is_none_or(|l| l == dst)
            && (dev == 0 || iif == 0 || dev == iif)
    }

    /// `raw_v4_input`/`ipv6_raw_deliver` with `raw_rcv`: a copy of every packet of the
    /// socket's family and protocol to its bound address, from its peer, on its device, of a
    /// group it allows, that its ICMP filter passes (`raw_v4_match`, `icmp_filter`,
    /// `icmpv6_filter`). An IPv4 socket gets the IP header too. Returns whether it matched
    /// (`raw_v4_input`'s `delivered`), whatever its receive buffer then made of it.
    pub(crate) fn raw_input(&self, p: &crate::raw::Packet, iif: u32, pkttype: u8) -> bool {
        if !self.raw || p.dst.is_ipv6() != self.v6() || p.proto != self.proto {
            return false;
        }
        let icmp = i32::from(self.proto) == IPPROTO_ICMP || i32::from(self.proto) == IPPROTO_ICMPV6;
        let g = self.lock();
        let dev = self.device(&g);
        let matches = g.local.is_none_or(|l| l == p.dst)
            && g.peer.is_none_or(|peer| peer == p.src)
            && (dev == 0 || dev == iif)
            && (!p.dst.is_multicast() || g.opts.mc.allows(p.dst, p.src, iif));
        let filtered =
            icmp && (p.data.len() < ICMP_HDR || filtered(&g.filter, self.v6(), p.data[0]));
        drop(g);
        if !matches || filtered {
            return matches;
        }
        let data = match (p.src, p.dst) {
            (IpAddr::V4(s), IpAddr::V4(d)) => {
                let id = self.net.state().icmp.next_ip_id();
                let h = ipv4_header(s, d, p.proto, p.ttl, p.data.len(), id);
                [&h[..], &p.data].concat()
            }
            _ => p.data.clone(),
        };
        let msg = IcmpMsg {
            src: p.src,
            dst: p.dst,
            ttl: p.ttl,
            data: Vec::new(),
        };
        self.queue(data, &msg, iif, pkttype);
        true
    }

    /// `sk_filter`: whether the socket's filter keeps `data`, a message that arrived as `at`
    /// says, and how much of it (an IPv4 raw socket's filter sees the IP header too).
    fn filter(&self, filter: &SockFilter, data: &mut Vec<u8>, at: &Arrival) -> bool {
        if !filter.attached() {
            return true;
        }
        let proto = self.protocol() as u8;
        let frame = match self.raw && !self.v6() && data.len() >= IPV4_HDR {
            true => self
                .net
                .frame(at, proto, &data[..IPV4_HDR], &data[IPV4_HDR..], true),
            false => self.net.frame(at, proto, &[], data, false),
        };
        match filter.trim(&frame.packet(), 1) {
            Some(keep) => {
                data.truncate(keep);
                true
            }
            None => false,
        }
    }

    /// Queues a message that passes the socket's filter, dropped when the receive buffer is
    /// full (`sock_queue_rcv_skb`).
    fn queue(&self, mut data: Vec<u8>, msg: &IcmpMsg, iif: u32, pkttype: u8) {
        let filter = self.lock().opts.filter.clone();
        let at = Arrival {
            src: msg.src,
            dst: msg.dst,
            ttl: msg.ttl,
            traffic_class: 0,
            iif,
            pkttype,
        };
        if !self.filter(&filter, &mut data, &at) {
            return;
        }
        let mut g = self.lock();
        if g.queued > g.opts.rcvbuf {
            g.drops = g.drops.wrapping_add(1);
            return;
        }
        g.queued += data.len() + OVERHEAD;
        g.queue.push_back(Received {
            data,
            from: msg.src,
            dst: msg.dst,
            iif,
            ttl: msg.ttl,
        });
        drop(g);
        self.wq.wake(PollMask::IN | PollMask::RDNORM);
    }

    /// `ping_err`: with `IP_RECVERR` the error is queued and becomes the pending one, without
    /// only a hard error to a connected socket is.
    fn ping_error(&self, q: &Quoted<'_>) {
        let mut g = self.lock();
        let recverr = match self.domain {
            Domain::Inet => g.opts.flag(SOL_IP, IpOption::IP_RECVERR.0),
            Domain::Inet6 => g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVERR.0),
        };
        if recverr {
            let e = QueuedError {
                errno: q.errno,
                origin: match self.domain {
                    Domain::Inet => SoEeOrigin::SO_EE_ORIGIN_ICMP,
                    Domain::Inet6 => SoEeOrigin::SO_EE_ORIGIN_ICMP6,
                },
                type_: q.type_,
                code: q.code,
                info: q.info,
                data: 0,
                offender: Some(SocketAddr::new(q.offender, 0)),
                dst: Some(SocketAddr::new(q.dst, 0)),
                payload: q.transport.to_vec(),
            };
            let size = e.payload.len() + OVERHEAD;
            if g.queued + size < g.opts.rcvbuf {
                g.queued += size;
                g.errqueue.push_back(e);
            }
        } else if !q.hard || g.peer.is_none() {
            return;
        }
        g.err = Some(q.errno);
        drop(g);
        self.wq.wake(PollMask::ERR);
    }

    /// `__ip_append_data`/`__ip6_append_data`'s size check of a message of `len` bytes to
    /// `dst` out a link of `mtu`: `EMSGSIZE` when it may not be fragmented and does not fit,
    /// with a local error naming the MTU queued under `IP_RECVERR` (`ip_local_error`,
    /// `ipv6_local_error`). What `IP_HDRINCL` sends is never fragmented and already holds its
    /// IP header (`raw_send_hdrinc`). With `IPV6_DONTFRAG`, the path MTU is also left for
    /// `IPV6_RECVPATHMTU` (`ipv6_local_rxpmtu`), with the flow's interface `oif`.
    fn check_mtu(
        &self,
        g: &mut Inner,
        dst: IpAddr,
        oif: u32,
        mtu: Option<u32>,
        len: usize,
        dontfrag: Option<bool>,
    ) -> NetResult<()> {
        let Some(mtu) = mtu.map(|m| g.opts.fragment_mtu(m, self.v6()) as usize) else {
            return Ok(());
        };
        let v6 = self.v6();
        let fits = match self.raw && g.hdrincl {
            true => len <= mtu,
            false => {
                let header = if v6 { IPV6_HDR } else { IPV4_HDR };
                len + header <= mtu || !g.opts.dont_fragment(v6, dontfrag)
            }
        };
        if fits {
            return Ok(());
        }
        // `__ip6_append_data`; a header the socket writes itself goes by `rawv6_send_hdrinc`.
        if let (IpAddr::V6(v6), false) = (dst, self.raw && g.hdrincl) {
            g.opts.local_rxpmtu(v6, oif, mtu as u32, dontfrag);
        }
        let recverr = match v6 {
            false => g.opts.flag(SOL_IP, IpOption::IP_RECVERR.0),
            true => g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVERR.0),
        };
        if recverr && g.queued + OVERHEAD < g.opts.rcvbuf {
            g.queued += OVERHEAD;
            let e = QueuedError::local(Errno::EMSGSIZE, mtu as u32, dst, g.dport);
            g.errqueue.push_back(e);
        }
        Err(Errno::EMSGSIZE)
    }

    /// The pending error, else the backend endpoint's.
    fn take_err(g: &mut Inner) -> Option<Errno> {
        g.err
            .take()
            .or_else(|| g.backend.as_ref().and_then(|(b, _)| b.take_error()))
    }

    /// The backend's ICMP endpoint, made on first use; one the backend cannot make is
    /// `EPERM`, as a packet filter would refuse the echo.
    fn backend_endpoint(&self, g: &mut Inner, src: IpAddr) -> NetResult<Arc<dyn BackendDatagram>> {
        if let Some((b, _)) = &g.backend {
            let b = b.clone();
            g.backend.as_mut().expect("present").1 = src;
            return Ok(b);
        }
        let backend = self.net.backend.as_ref().ok_or(Errno::ENETUNREACH)?;
        let b = backend
            .ping(self.domain, self.wq.clone())
            .map_err(|e| match e {
                Errno::EACCES | Errno::EPERM | Errno::EPROTONOSUPPORT | Errno::EAFNOSUPPORT => {
                    Errno::EPERM
                }
                e => e,
            })?;
        for (level, name) in [
            (SOL_IP, IpOption::IP_RECVERR.0),
            (SOL_IPV6, Ipv6Option::IPV6_RECVERR.0),
        ] {
            if g.opts.flag(level, name) {
                let _ = b.set_option(level, name, &1i32.to_le_bytes());
            }
        }
        g.backend = Some((b.clone(), src));
        Ok(b)
    }

    /// A reply the backend received, as the guest sees it: the identifier the guest sent
    /// with, its checksum again, and for a raw IPv4 socket an IPv4 header.
    fn backend_reply(&self, g: &Inner, mut data: Vec<u8>, from: SocketAddr) -> Option<Received> {
        let (_, local) = g.backend.as_ref()?;
        let from = canonical_ip(self.net.guest_view(from).ip());
        if data.len() < ICMP_HDR {
            return None;
        }
        let ident = match self.raw {
            true => g.echo_id,
            false => g.ident?,
        };
        data[4..6].copy_from_slice(&ident.to_be_bytes());
        set_checksum(from, *local, &mut data, 2);
        if self.raw && !self.v6() {
            let h = ipv4_header(
                v4(from),
                v4(*local),
                IPPROTO_ICMP as u8,
                DEFAULT_TTL,
                data.len(),
                0,
            );
            data = [&h[..], &data].concat();
        }
        Some(Received {
            data,
            from,
            dst: *local,
            iif: 0,
            ttl: DEFAULT_TTL,
        })
    }

    /// The destination of a send: the address given (of the socket's family; IPv4-mapped
    /// IPv6 addresses are not for ICMPv6), else the peer.
    fn destination(&self, g: &Inner, to: Option<SocketAddr>) -> NetResult<(IpAddr, u32)> {
        let Some(to) = to else {
            return g.peer.map(|p| (p, 0)).ok_or(Errno::EDESTADDRREQ);
        };
        match (self.domain, to) {
            (Domain::Inet, SocketAddr::V4(v4)) => Ok((IpAddr::V4(*v4.ip()), 0)),
            (Domain::Inet6, SocketAddr::V6(v6)) if v6.ip().to_ipv4_mapped().is_some() => {
                Err(match self.raw {
                    true => Errno::ENETUNREACH,
                    false => Errno::EINVAL,
                })
            }
            (Domain::Inet6, SocketAddr::V6(v6)) => Ok((IpAddr::V6(*v6.ip()), v6.scope_id())),
            _ => Err(Errno::EAFNOSUPPORT),
        }
    }

    /// `IP_TTL`/`IPV6_UNICAST_HOPS`, or the default.
    fn unicast_ttl(&self, g: &Inner) -> u8 {
        g.opts.unicast_ttl(self.domain)
    }

    /// The bound address as a source, unless it is a group's.
    fn bound_src(g: &Inner) -> Option<IpAddr> {
        g.local.filter(|ip| !ip.is_multicast())
    }

    /// `ip_cmsg_recv`, `ip6_datagram_recv_specific_ctl`: the control messages the socket
    /// asked for, in Linux's order.
    fn controls(&self, g: &Inner, r: &Received) -> Vec<Control> {
        let mut out = Vec::new();
        let int = |level, kind, v: i32| Control {
            level,
            kind,
            data: v.to_le_bytes().to_vec(),
        };
        match self.domain {
            Domain::Inet => {
                if g.opts.flag(SOL_IP, IpOption::IP_PKTINFO.0) {
                    let spec = match r.dst.is_multicast() {
                        true => self.net.state().ifaces.source_on(r.iif, r.from),
                        false => Some(r.dst),
                    };
                    let data = cmsg::in_pktinfo(r.iif, v4(spec.unwrap_or(r.dst)), v4(r.dst));
                    out.push(Control {
                        level: SOL_IP,
                        kind: IpOption::IP_PKTINFO.0,
                        data,
                    });
                }
                if g.opts.flag(SOL_IP, IpOption::IP_RECVTTL.0) {
                    out.push(int(SOL_IP, IpOption::IP_TTL.0, i32::from(r.ttl)));
                }
                if g.opts.flag(SOL_IP, IpOption::IP_RECVTOS.0) {
                    out.push(Control {
                        level: SOL_IP,
                        kind: IpOption::IP_TOS.0,
                        data: vec![0],
                    });
                }
            }
            Domain::Inet6 => {
                if g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVPKTINFO.0) {
                    let data = cmsg::in6_pktinfo(v6(r.dst), r.iif);
                    out.push(Control {
                        level: SOL_IPV6,
                        kind: Ipv6Option::IPV6_PKTINFO.0,
                        data,
                    });
                }
                if g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVHOPLIMIT.0) {
                    out.push(int(SOL_IPV6, Ipv6Option::IPV6_HOPLIMIT.0, i32::from(r.ttl)));
                }
                // net/ipv6/datagram.c ip6_datagram_recv_common_ctl: RFC2292
                // uses its own receive bits and ancillary types.
                if g.opts.flag(SOL_IPV6, crate::opts::IPV6_2292PKTINFO) {
                    out.push(Control {
                        level: SOL_IPV6,
                        kind: crate::opts::IPV6_2292PKTINFO,
                        data: cmsg::in6_pktinfo(v6(r.dst), r.iif),
                    });
                }
                if g.opts.flag(SOL_IPV6, crate::opts::IPV6_2292HOPLIMIT) {
                    out.push(int(
                        SOL_IPV6,
                        crate::opts::IPV6_2292HOPLIMIT,
                        i32::from(r.ttl),
                    ));
                }
                if g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVTCLASS.0) {
                    out.push(int(SOL_IPV6, Ipv6Option::IPV6_TCLASS.0, 0));
                }
            }
        }
        out
    }

    /// The sender as `recvfrom` reports it: port 0, and for a link-local IPv6 sender the
    /// interface it arrived on as the scope.
    fn sender(&self, r: &Received) -> SocketAddr {
        match r.from {
            IpAddr::V6(v6) if v6.is_unicast_link_local() => {
                SocketAddr::V6(SocketAddrV6::new(v6, 0, 0, r.iif))
            }
            ip => present(self.domain, SocketAddr::new(ip, 0)),
        }
    }

    /// `ip_recv_error` / `ipv6_recv_error`.
    fn recv_error(&self, buf: &mut [u8]) -> NetResult<RecvMsg> {
        let mut g = self.lock();
        let e = match g.errqueue.pop_front() {
            Some(e) => {
                g.queued -= e.payload.len() + OVERHEAD;
                match g.errqueue.front() {
                    Some(next) if next.is_icmp() => g.err = Some(next.errno),
                    _ if e.is_icmp() => g.err = None,
                    _ => {}
                }
                e
            }
            None => {
                let backend = g.backend.clone();
                drop(g);
                let mut e = backend
                    .and_then(|(b, _)| b.take_queued_error())
                    .ok_or(Errno::EAGAIN)?;
                e.dst = e.dst.map(|a| self.net.guest_view(a));
                e.offender = e.offender.map(|a| self.net.guest_view(a));
                e
            }
        };
        Ok(error_result(self.domain, &e, buf))
    }

    /// `raw_send_hdrinc`, `rawv6_send_hdrinc`: a raw socket with `IP_HDRINCL` (`IPV6_HDRINCL`)
    /// sends the IP header it is given: its source (if any), TTL and protocol, and what
    /// follows. `EINVAL` for a short or malformed header.
    fn strip_header(domain: Domain, data: &[u8]) -> NetResult<(Option<IpAddr>, u8, u8, &[u8])> {
        if domain == Domain::Inet6 {
            if data.len() < IPV6_HDR {
                return Err(Errno::EINVAL);
            }
            let src = Ipv6Addr::from(<[u8; 16]>::try_from(&data[8..24]).expect("16 bytes"));
            let src = (!src.is_unspecified()).then_some(IpAddr::V6(src));
            return Ok((src, data[7], data[6], &data[IPV6_HDR..]));
        }
        let ihl = usize::from(*data.first().ok_or(Errno::EINVAL)? & 0x0f) * 4;
        if data.len() < IPV4_HDR || ihl < IPV4_HDR || ihl > data.len() {
            return Err(Errno::EINVAL);
        }
        let src = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
        let src = (!src.is_unspecified()).then_some(IpAddr::V4(src));
        Ok((src, data[8], data[9], &data[ihl..]))
    }
}

/// `icmp_filter`, `icmpv6_filter`: whether the socket's filter blocks `type_`.
fn filtered(filter: &[u32; 8], v6: bool, type_: u8) -> bool {
    match v6 {
        false => type_ < 32 && filter[0] & (1 << type_) != 0,
        true => filter[usize::from(type_ >> 5)] & (1 << (type_ & 31)) != 0,
    }
}

impl Drop for IcmpSocket {
    fn drop(&mut self) {
        let g = self.inner.get_mut().unwrap_or_else(|e| e.into_inner());
        g.opts.mc.drop_all(&self.net);
        let mut st = self.net.state();
        st.icmp.unbind(self.id);
        st.icmp.raw.retain(|w| w.strong_count() > 0);
    }
}

impl Pollable for IcmpSocket {
    /// `datagram_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wq);
        let g = self.lock();
        let mut m = PollMask::empty();
        if g.err.is_some() || !g.errqueue.is_empty() {
            m |= PollMask::ERR;
        }
        if g.rd_shut {
            m |= PollMask::RDHUP | PollMask::IN | PollMask::RDNORM;
        }
        if g.rd_shut && g.wr_shut {
            m |= PollMask::HUP;
        }
        if !g.queue.is_empty() {
            m |= PollMask::IN | PollMask::RDNORM;
        }
        let backend = g.backend.clone();
        drop(g);
        let mut writable = true;
        if let Some((b, _)) = backend {
            let bm = b.poll(None);
            m |= bm & (PollMask::IN | PollMask::RDNORM | PollMask::ERR);
            writable = bm.contains(PollMask::OUT);
        }
        if writable {
            m |= PollMask::OUT | PollMask::WRNORM | PollMask::WRBAND;
        }
        m
    }
}

impl Socket for IcmpSocket {
    fn domain(&self) -> Domain {
        self.domain
    }

    fn kind(&self) -> SocketKind {
        match self.raw {
            true => SocketKind::Raw,
            false => SocketKind::Datagram,
        }
    }

    fn protocol(&self) -> i32 {
        i32::from(self.proto)
    }

    /// `ping_bind` (`ping_check_bind_addr`, `ping_get_port` with the port as identifier),
    /// `raw_bind`, `rawv6_bind`.
    fn bind(&self, addr: SocketAddr, _privileged: bool) -> NetResult<()> {
        let mapped = matches!(addr, SocketAddr::V6(v6) if v6.ip().to_ipv4_mapped().is_some());
        if self.v6() && mapped {
            return Err(match self.raw {
                true => Errno::EADDRNOTAVAIL,
                false => Errno::EINVAL,
            });
        }
        let ip = addr.ip();
        if self.v6() && !self.raw && ip.is_multicast() {
            return Err(Errno::EINVAL);
        }
        let mut g = self.lock();
        let freebind = match self.domain {
            Domain::Inet => g.opts.flag(SOL_IP, IpOption::IP_FREEBIND.0),
            Domain::Inet6 => g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_FREEBIND.0),
        };
        let usable = ip.is_unspecified()
            || ip.is_multicast()
            || ip == IpAddr::V4(Ipv4Addr::BROADCAST)
            || freebind
            || self.net.state().ifaces.is_local(ip);
        if !usable {
            return Err(Errno::EADDRNOTAVAIL);
        }
        if !self.raw {
            if g.ident.is_some() {
                return Err(Errno::EINVAL);
            }
            let ident = self.net.state().icmp.bind(
                self.id,
                addr.port(),
                g.opts.reuseaddr,
                self.me.clone(),
            )?;
            g.ident = Some(ident);
        } else if g.peer.is_some() {
            return Err(Errno::EINVAL);
        }
        g.local = (!ip.is_unspecified()).then_some(ip);
        g.addr_locked = g.local.is_some();
        Ok(())
    }

    fn listen(&self, _backlog: u32) -> NetResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    fn accept(&self) -> NetResult<Accepted> {
        Err(Errno::EOPNOTSUPP)
    }

    /// `ip4_datagram_connect`, `ip6_datagram_connect`: the peer, and the route's source as
    /// the bound address if the socket had none; a ping socket takes an identifier first.
    fn connect(&self, addr: SocketAddr) -> NetResult<()> {
        let mut g = self.lock();
        if !self.raw {
            self.autobind(&mut g).map_err(|_| Errno::EAGAIN)?;
        }
        let (dst, scope) = match (self.domain, addr) {
            (Domain::Inet6, SocketAddr::V4(v4)) => (IpAddr::V4(*v4.ip()), 0),
            _ => self.destination(&g, Some(addr))?,
        };
        let src = match dst.is_multicast() {
            true => {
                let (oif, mc_src) = g.opts.mc.interface(dst);
                let oif = [scope, self.device(&g), oif]
                    .into_iter()
                    .find(|&i| i != 0)
                    .unwrap_or(0);
                self.net
                    .mc_route(dst, oif, Self::bound_src(&g).or(mc_src))?
                    .src
            }
            false => self.net.ip_path(dst, Self::bound_src(&g))?.src(),
        };
        if g.local.is_none() && !src.is_unspecified() {
            g.local = Some(src);
        }
        g.peer = Some(dst);
        g.dport = addr.port();
        Ok(())
    }

    fn disconnect(&self) -> NetResult<()> {
        let mut g = self.lock();
        g.peer = None;
        g.dport = 0;
        if !g.addr_locked {
            g.local = None;
        }
        Ok(())
    }

    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        let mut g = self.lock();
        g.rd_shut |= how != Shutdown::Write;
        g.wr_shut |= how != Shutdown::Read;
        let connected = g.peer.is_some();
        drop(g);
        self.wq.wake(PollMask::empty());
        match connected {
            true => Ok(()),
            false => Err(Errno::ENOTCONN),
        }
    }

    /// `ping_v4_sendmsg`/`ping_v6_sendmsg` (an echo request; the kernel sets the identifier
    /// and checksum), `raw_sendmsg`/`rawv6_sendmsg` (any ICMP message as given, ICMPv6's
    /// checksum computed), failing in their order.
    fn send(&self, msg: SendMsg<'_>) -> NetResult<usize> {
        let mut g = self.lock();
        let ident = match self.raw {
            true => 0,
            false => self.autobind(&mut g).map_err(|_| Errno::EAGAIN)?,
        };
        if msg.data.len() > 0xFFFF {
            return Err(Errno::EMSGSIZE);
        }
        if msg.flags.0 & MsgFlags::MSG_OOB.0 != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        let echo = match self.domain {
            Domain::Inet => ICMP_ECHO,
            Domain::Inet6 => ICMPV6_ECHO_REQUEST,
        };
        // `ping_common_sendmsg`: an echo request header at least.
        if !self.raw && (msg.data.len() < ICMP_HDR || msg.data[0] != echo || msg.data[1] != 0) {
            return Err(Errno::EINVAL);
        }
        let (dst, scope) = self.destination(&g, msg.to)?;
        let ctl: SendCtl = cmsg::inet(msg.control, self.domain, dst.is_ipv4(), msg.net_raw)?;
        let hdrincl = self.raw && g.hdrincl;
        let (mut data, hdr_src, hdr_ttl, proto) = match hdrincl {
            true => {
                let (src, ttl, proto, rest) = Self::strip_header(self.domain, msg.data)?;
                (rest.to_vec(), src, Some(ttl), proto)
            }
            false => (msg.data.to_vec(), None, None, self.proto),
        };
        let icmp = match self.domain {
            Domain::Inet => IPPROTO_ICMP,
            Domain::Inet6 => IPPROTO_ICMPV6,
        } as u8;
        // `ping_v6_sendmsg`: a link-local destination needs an interface.
        let (sticky_src, sticky_oif) = match self.domain {
            Domain::Inet6 => g.opts.sticky_pktinfo,
            Domain::Inet => (Ipv6Addr::UNSPECIFIED, 0),
        };
        let oif = [ctl.oif, scope, self.device(&g), sticky_oif]
            .into_iter()
            .find(|&i| i != 0)
            .unwrap_or(0);
        if let IpAddr::V6(v6) = dst
            && !self.raw
            && oif == 0
            && v6.is_unicast_link_local()
        {
            return Err(Errno::EINVAL);
        }
        let sticky_src = Some(IpAddr::V6(sticky_src)).filter(|ip| !ip.is_unspecified());
        let src_wanted = hdr_src
            .or(ctl.src.map(canonical_ip))
            .or(sticky_src)
            .or(Self::bound_src(&g));
        if !dst.is_multicast() {
            if dst == IpAddr::V4(Ipv4Addr::BROADCAST)
                && !g.opts.flag(SOL_SOCKET, SocketOption::SO_BROADCAST.0)
            {
                return Err(Errno::EACCES);
            }
            let path = self.net.ip_path(dst, src_wanted)?;
            let mtu = self.net.mtu_to(dst);
            self.check_mtu(&mut g, dst, oif, mtu, msg.data.len(), ctl.dontfrag)?;
            let ttl = hdr_ttl.or(ctl.ttl).unwrap_or_else(|| self.unicast_ttl(&g));
            if proto != icmp {
                let p = crate::raw::Packet {
                    src: path.src(),
                    dst,
                    ttl,
                    proto,
                    data,
                };
                return self.send_packet(g, p, hdrincl, path, msg.data.len(), msg.now);
            }
            return self.send_unicast(g, data, dst, ttl, ident, path, msg.data.len(), msg.now);
        }
        let (mc_oif, mc_src) = g.opts.mc.interface(dst);
        let oif = if oif != 0 { oif } else { mc_oif };
        let route = self.net.mc_route(dst, oif, src_wanted.or(mc_src))?;
        let mtu = self.net.link_mtu(route.oif);
        self.check_mtu(&mut g, dst, oif, mtu, msg.data.len(), ctl.dontfrag)?;
        let (ttl, looped) = g.opts.mc.ttl_loop(dst);
        let ttl = hdr_ttl.or(ctl.ttl).unwrap_or(ttl);
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        if proto != icmp {
            self.raw_checksum(&g, &mut data, route.src, dst, proto, hdrincl)?;
            drop(g);
            let packet = crate::raw::Packet {
                src: route.src,
                dst,
                ttl,
                proto,
                data,
            };
            for (net, iif) in self
                .net
                .mc_receivers(dst, route.src, route.oif, looped, ttl)
            {
                let back = Back {
                    net: self.net.clone(),
                    iif: route.oif,
                };
                net.ip_input(packet.clone(), iif, back, msg.now);
            }
            return Ok(msg.data.len());
        }
        self.finish(&g, &mut data, route.src, dst, ident)?;
        drop(g);
        let message = IcmpMsg {
            src: route.src,
            dst,
            ttl,
            data,
        };
        for (net, iif) in self
            .net
            .mc_receivers(dst, route.src, route.oif, looped, ttl)
        {
            let back = Back {
                net: self.net.clone(),
                iif: route.oif,
            };
            net.icmp_input(message.clone(), iif, back, msg.now);
        }
        Ok(msg.data.len())
    }

    /// `ping_recvmsg`, `raw_recvmsg`, `rawv6_recvmsg`.
    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> NetResult<RecvMsg> {
        if flags.0 & MsgFlags::MSG_OOB.0 != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        if flags.contains(MsgFlags::MSG_ERRQUEUE) {
            return self.recv_error(buf);
        }
        let peek = flags.0 & MsgFlags::MSG_PEEK.0 != 0;
        let mut g = self.lock();
        // `rawv6_recvmsg`; `ping_recvmsg` never looks.
        if self.raw
            && let Some(r) = g.opts.recv_rxpmtu()
        {
            return Ok(r);
        }
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.queue.is_empty() {
            let backend = g.backend.clone();
            if let Some((b, _)) = backend {
                let mut tmp = vec![0; 0xFFFF];
                match b.recv_from(&mut tmp, false) {
                    Ok((n, _, from)) => {
                        tmp.truncate(n);
                        if let Some(mut r) = self.backend_reply(&g, tmp, from) {
                            let at = Arrival {
                                src: r.from,
                                dst: r.dst,
                                ttl: r.ttl,
                                traffic_class: 0,
                                iif: self.net.uplink_index(),
                                pkttype: PACKET_HOST,
                            };
                            r.iif = at.iif;
                            let filter = g.opts.filter.clone();
                            if self.filter(&filter, &mut r.data, &at) {
                                g.queued += r.data.len() + OVERHEAD;
                                g.queue.push_back(r);
                            }
                        }
                    }
                    Err(Errno::EAGAIN) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let Some(r) = g.queue.front() else {
            return match g.rd_shut {
                true => Ok(RecvMsg::default()),
                false => Err(Errno::EAGAIN),
            };
        };
        let full = r.data.len();
        let len = buf.len().min(full);
        buf[..len].copy_from_slice(&r.data[..len]);
        let from = self.sender(r);
        let control = self.controls(&g, r);
        if !peek {
            g.queue.pop_front();
            g.queued -= full + OVERHEAD;
        }
        Ok(RecvMsg {
            len,
            full_len: full,
            from: Some(from),
            flags: match full > len {
                true => MsgFlags::MSG_TRUNC,
                false => MsgFlags(0),
            },
            control,
        })
    }

    fn local_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        let port = g.ident.unwrap_or(0);
        Ok(match g.local {
            Some(ip) => present(self.domain, SocketAddr::new(ip, port)),
            None if port != 0 => SocketAddr::new(self.any(), port),
            None => unbound(self.domain),
        })
    }

    fn peer_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        g.peer
            .map(|p| present(self.domain, SocketAddr::new(p, 0)))
            .ok_or(Errno::ENOTCONN)
    }

    fn take_error(&self) -> Option<Errno> {
        Self::take_err(&mut self.lock())
    }

    fn input_len(&self) -> NetResult<usize> {
        let g = self.lock();
        if let Some(r) = g.queue.front() {
            return Ok(r.data.len());
        }
        Ok(g.backend.as_ref().map_or(0, |(b, _)| b.input_len()))
    }

    /// `do_raw_setsockopt` (`ICMP_FILTER`), `do_rawv6_setsockopt` (`IPV6_CHECKSUM`),
    /// `rawv6_seticmpfilter` (`ICMP6_FILTER`), `IP_HDRINCL`, then the IP options.
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        let mut g = self.lock();
        let icmp = i32::from(self.proto) == IPPROTO_ICMP;
        let icmpv6 = i32::from(self.proto) == IPPROTO_ICMPV6;
        let hdrincl6 = Ipv6Option::IPV6_HDRINCL.0;
        match (level, self.raw, self.domain) {
            // `raw_seticmpfilter`, `rawv6_seticmpfilter`: for ICMP sockets only.
            (SOL_RAW, true, Domain::Inet) if !icmp => return Err(Errno::EOPNOTSUPP),
            (SOL_ICMPV6, true, Domain::Inet6) if !icmpv6 => return Err(Errno::EOPNOTSUPP),
            (SOL_RAW, true, Domain::Inet) => {
                if name != ICMP_FILTER {
                    return Err(Errno::ENOPROTOOPT);
                }
                // A short value sets that much of the filter.
                let mut bytes = g.filter[0].to_le_bytes();
                let n = value.len().min(4);
                bytes[..n].copy_from_slice(&value[..n]);
                g.filter[0] = u32::from_le_bytes(bytes);
                return Ok(());
            }
            // `do_rawv6_setsockopt`, reached at `SOL_IPV6` too for these two options; RFC 3542
            // keeps an ICMPv6 socket's checksum to `SOL_RAW`.
            (SOL_RAW, true, Domain::Inet6) | (SOL_IPV6, true, Domain::Inet6)
                if level == SOL_RAW || name == IPV6_CHECKSUM || name == hdrincl6 =>
            {
                if value.len() < 4 {
                    return Err(Errno::EINVAL);
                }
                let val = read_int(value)?;
                return match name {
                    IPV6_CHECKSUM if icmpv6 && level == SOL_IPV6 => Err(Errno::EINVAL),
                    IPV6_CHECKSUM if val > 0 && val & 1 != 0 => Err(Errno::EINVAL),
                    IPV6_CHECKSUM => {
                        g.csum_offset = usize::try_from(val).ok();
                        Ok(())
                    }
                    n if n == hdrincl6 => {
                        g.hdrincl = val != 0;
                        Ok(())
                    }
                    _ => Err(Errno::ENOPROTOOPT),
                };
            }
            (SOL_ICMPV6, true, Domain::Inet6) => {
                if name != ICMP6_FILTER {
                    return Err(Errno::ENOPROTOOPT);
                }
                let mut bytes = [0u8; 32];
                for (i, w) in g.filter.iter().enumerate() {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
                }
                let n = value.len().min(32);
                bytes[..n].copy_from_slice(&value[..n]);
                for (i, w) in g.filter.iter_mut().enumerate() {
                    *w = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("4"));
                }
                return Ok(());
            }
            (SOL_RAW | SOL_ICMPV6, ..) => return Err(Errno::ENOPROTOOPT),
            // A raw IPv6 socket has no IPv4 options.
            (SOL_IP, true, Domain::Inet6) => return Err(Errno::ENOPROTOOPT),
            (SOL_IP, true, Domain::Inet) if name == IpOption::IP_HDRINCL.0 => {
                g.hdrincl = match value.len() {
                    0 => false,
                    1..4 => value[0] != 0,
                    _ => read_int(value)? != 0,
                };
                return Ok(());
            }
            _ => {}
        }
        if crate::mcast::handles(level, name) {
            return g.opts.set_mcast(&self.net, level, name, value);
        }
        let bound = g.ident.is_some();
        g.opts.set(level, name, value, bound)?;
        let recverr = [
            (SOL_IP, IpOption::IP_RECVERR.0),
            (SOL_IPV6, Ipv6Option::IPV6_RECVERR.0),
        ];
        if recverr.contains(&(level, name)) {
            if !g.opts.flag(level, name) {
                let purged: usize = g
                    .errqueue
                    .drain(..)
                    .map(|e| e.payload.len() + OVERHEAD)
                    .sum();
                g.queued -= purged;
            }
            if let Some((b, _)) = &g.backend {
                let on = i32::from(g.opts.flag(level, name));
                let _ = b.set_option(level, name, &on.to_le_bytes());
            }
        }
        Ok(())
    }

    fn get_option(&self, level: i32, name: i32, out: &mut [u8]) -> NetResult<usize> {
        let mut g = self.lock();
        let icmp = i32::from(self.proto) == IPPROTO_ICMP;
        let icmpv6 = i32::from(self.proto) == IPPROTO_ICMPV6;
        let hdrincl6 = Ipv6Option::IPV6_HDRINCL.0;
        match (level, self.raw, self.domain) {
            (SOL_RAW, true, Domain::Inet) if !icmp => return Err(Errno::EOPNOTSUPP),
            (SOL_ICMPV6, true, Domain::Inet6) if !icmpv6 => return Err(Errno::EOPNOTSUPP),
            (SOL_RAW, true, Domain::Inet) if name == ICMP_FILTER => {
                return Ok(write_bytes(out, &g.filter[0].to_le_bytes()));
            }
            (SOL_RAW | SOL_IPV6, true, Domain::Inet6) if name == IPV6_CHECKSUM => {
                let v = g.csum_offset.map_or(-1, |o| o as i32);
                return Ok(write_int(out, v));
            }
            (SOL_RAW | SOL_IPV6, true, Domain::Inet6) if name == hdrincl6 => {
                return Ok(write_int(out, i32::from(g.hdrincl)));
            }
            (SOL_ICMPV6, true, Domain::Inet6) if name == ICMP6_FILTER => {
                let bytes: Vec<u8> = g.filter.iter().flat_map(|w| w.to_le_bytes()).collect();
                return Ok(write_bytes(out, &bytes));
            }
            (SOL_RAW | SOL_ICMPV6, ..) => return Err(Errno::ENOPROTOOPT),
            (SOL_IP, true, Domain::Inet) if name == IpOption::IP_HDRINCL.0 => {
                return Ok(write_int(out, i32::from(g.hdrincl)));
            }
            _ => {}
        }
        if level == SOL_SOCKET && name == SocketOption::SO_ERROR.0 {
            let e = Self::take_err(&mut g).map_or(0, |e| e.0);
            return Ok(write_int(out, e));
        }
        if level == SOL_SOCKET && name == SocketOption::SO_ACCEPTCONN.0 {
            return Ok(write_int(out, 0));
        }
        if crate::mcast::handles(level, name) {
            return g.opts.get_mcast(&self.net, level, name, out);
        }
        if let Some(result) = g.opts.get_mtu(&self.net, g.peer, level, name, out) {
            return result;
        }
        g.opts.get(level, name, out)
    }

    fn graft(&self, ino: u64) {
        self.ino.store(ino, Ordering::Relaxed);
    }
}

impl IcmpSocket {
    /// The identifier and checksum the kernel gives what it sends: a ping socket's echo gets
    /// the socket's identifier; ICMPv6's checksum is the kernel's (ping and raw sockets alike,
    /// at `IPV6_CHECKSUM`'s offset), IPv4's only for a ping socket.
    fn finish(
        &self,
        g: &Inner,
        data: &mut [u8],
        src: IpAddr,
        dst: IpAddr,
        ident: u16,
    ) -> NetResult<()> {
        if !self.raw {
            data[4..6].copy_from_slice(&ident.to_be_bytes());
            set_checksum(src, dst, data, 2);
            return Ok(());
        }
        if let (Domain::Inet6, Some(offset)) = (self.domain, g.csum_offset) {
            // `rawv6_push_pending_frames`.
            if offset + 1 >= data.len() {
                return Err(Errno::EINVAL);
            }
            set_checksum(src, dst, data, offset);
        }
        Ok(())
    }

    /// The checksum a raw IPv6 socket's `IPV6_CHECKSUM` asks for, over a packet of `proto`
    /// it did not write the header of (`rawv6_push_pending_frames`).
    fn raw_checksum(
        &self,
        g: &Inner,
        data: &mut [u8],
        src: IpAddr,
        dst: IpAddr,
        proto: u8,
        hdrincl: bool,
    ) -> NetResult<()> {
        if let (Domain::Inet6, Some(offset), false) = (self.domain, g.csum_offset, hdrincl) {
            if offset + 1 >= data.len() {
                return Err(Errno::EINVAL);
            }
            set_transport_checksum(src, dst, proto, data, offset);
        }
        Ok(())
    }

    /// A raw packet of a protocol other than ICMP along `path`, of the `len` bytes the guest
    /// sent at `now`: to the namespace that holds its destination, where raw sockets and the
    /// protocol take it ([`Net::ip_input`]); nothing raw leaves for the host.
    fn send_packet(
        &self,
        mut g: MutexGuard<'_, Inner>,
        mut p: crate::raw::Packet,
        hdrincl: bool,
        path: IpPath,
        len: usize,
        now: Duration,
    ) -> NetResult<usize> {
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        self.raw_checksum(&g, &mut p.data, p.src, p.dst, p.proto, hdrincl)?;
        drop(g);
        match path {
            IpPath::Local {
                net, iif, back_iif, ..
            } => {
                let back = Back {
                    net: self.net.clone(),
                    iif: back_iif,
                };
                net.ip_input(p, iif, back, now);
            }
            IpPath::Nowhere {
                unresolved: true, ..
            } => self
                .net
                .neighbour_failed(p.src, p.dst, p.proto, &p.data, now),
            _ => {}
        }
        Ok(len)
    }

    /// A unicast send along `path`, from its source, of the `len` bytes the guest sent at
    /// `now`.
    #[allow(clippy::too_many_arguments)]
    fn send_unicast(
        &self,
        mut g: MutexGuard<'_, Inner>,
        mut data: Vec<u8>,
        dst: IpAddr,
        ttl: u8,
        ident: u16,
        path: IpPath,
        len: usize,
        now: Duration,
    ) -> NetResult<usize> {
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        let src = path.src();
        self.finish(&g, &mut data, src, dst, ident)?;
        if let IpPath::Dns { src } = path {
            drop(g);
            let msg = IcmpMsg {
                src,
                dst,
                ttl,
                data,
            };
            self.net.dns_icmp(msg, now);
            return Ok(len);
        }
        if let IpPath::Backend { host_dst, src } = path {
            let echo = match self.domain {
                Domain::Inet => ICMP_ECHO,
                Domain::Inet6 => ICMPV6_ECHO_REQUEST,
            };
            // The host's ICMP socket carries echo requests only; anything else is lost.
            if data.len() < ICMP_HDR || data[0] != echo {
                return Ok(len);
            }
            let b = self.backend_endpoint(&mut g, src)?;
            g.echo_id = u16::from_be_bytes([data[4], data[5]]);
            drop(g);
            b.send_to(&data, host_dst)?;
            return Ok(len);
        }
        drop(g);
        let msg = IcmpMsg {
            src,
            dst,
            ttl,
            data,
        };
        self.net.icmp_deliver(msg, path, now);
        Ok(len)
    }
}
