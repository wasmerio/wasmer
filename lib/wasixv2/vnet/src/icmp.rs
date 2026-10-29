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
};

use wasixv2_abi::{Errno, IpOption, Ipv6Option, MsgFlags, SoEeOrigin, SocketLevel, SocketOption};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, BackendDatagram, Control, Domain, NetResult, QueuedError, RecvMsg, SendMsg, Shutdown,
    Socket, SocketKind,
    addr::{canonical_ip, present, unbound},
    cmsg::{self, SendCtl},
    net::{IpPath, Net},
    opts::{SockOpts, read_int, write_bytes, write_int},
    ports::SockId,
    sync::MutexExt,
    udp::error_result,
};

pub(crate) const IPPROTO_ICMP: i32 = 1;
pub(crate) const IPPROTO_ICMPV6: i32 = 58;
const IPPROTO_UDP: u8 = 17;

const ICMP_ECHOREPLY: u8 = 0;
const ICMP_DEST_UNREACH: u8 = 3;
const ICMP_ECHO: u8 = 8;
const ICMP_TIME_EXCEEDED: u8 = 11;
const ICMP_PARAMETERPROB: u8 = 12;
pub(crate) const ICMP_PORT_UNREACH: u8 = 3;
const ICMPV6_DEST_UNREACH: u8 = 1;
const ICMPV6_PKT_TOOBIG: u8 = 2;
const ICMPV6_TIME_EXCEED: u8 = 3;
const ICMPV6_PARAMPROB: u8 = 4;
pub(crate) const ICMPV6_PORT_UNREACH: u8 = 4;
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
const IPV4_HDR: usize = 20;
const IPV6_HDR: usize = 40;
const UDP_HDR: usize = 8;
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
/// sockets, and the sysctls the kernel's ICMP heeds.
#[derive(Debug)]
pub(crate) struct IcmpTables {
    /// (identifier, socket, `SO_REUSEADDR`, socket), oldest first.
    ping: Vec<(u16, SockId, bool, Weak<IcmpSocket>)>,
    /// `ping_port_rover`: the last identifier given out.
    rover: u16,
    raw: Vec<Weak<IcmpSocket>>,
    /// `net.ipv4.icmp_echo_ignore_all`, `net.ipv4.icmp_echo_ignore_broadcasts`.
    pub echo_ignore_all: bool,
    pub echo_ignore_broadcasts: bool,
    /// The identification of the next IPv4 header made up for a raw socket.
    ip_id: u16,
}

impl Default for IcmpTables {
    fn default() -> Self {
        Self {
            ping: Vec::new(),
            rover: 0,
            raw: Vec::new(),
            echo_ignore_all: false,
            echo_ignore_broadcasts: true,
            ip_id: 1,
        }
    }
}

impl IcmpTables {
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

/// Writes the message's checksum (`csum_ipv6_magic`, `ip_compute_csum`) at `offset`.
fn set_checksum(src: IpAddr, dst: IpAddr, data: &mut [u8], offset: usize) {
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
fn ipv4_header(
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

fn ipv6_header(src: Ipv6Addr, dst: Ipv6Addr, proto: u8, hops: u8, len: usize) -> [u8; IPV6_HDR] {
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

fn v6(ip: IpAddr) -> Ipv6Addr {
    match ip {
        IpAddr::V6(v6) => v6,
        IpAddr::V4(v4) => v4.to_ipv6_mapped(),
    }
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
    let udp_len = UDP_HDR + payload.len();
    let mut udp = Vec::with_capacity(udp_len);
    udp.extend(src.port().to_be_bytes());
    udp.extend(dst.port().to_be_bytes());
    udp.extend((udp_len as u16).to_be_bytes());
    udp.extend([0, 0]);
    udp.extend_from_slice(payload);
    let csum = fold(pseudo_sum(src.ip(), dst.ip(), IPPROTO_UDP, udp_len) + sum16(&udp));
    udp[6..8].copy_from_slice(&csum.max(1).to_be_bytes());
    let (header, room): (Vec<u8>, usize) = match dst.ip() {
        IpAddr::V4(_) => (
            ipv4_header(
                v4(src.ip()),
                v4(dst.ip()),
                IPPROTO_UDP,
                DEFAULT_TTL,
                udp_len,
                0,
            )
            .to_vec(),
            ICMP_ERR_ROOM,
        ),
        IpAddr::V6(_) => (
            ipv6_header(
                v6(src.ip()),
                v6(dst.ip()),
                IPPROTO_UDP,
                DEFAULT_TTL,
                udp_len,
            )
            .to_vec(),
            ICMP6_ERR_ROOM,
        ),
    };
    let mut data = vec![type_, code, 0, 0, 0, 0, 0, 0];
    data.extend(header.iter().chain(&udp).take(room));
    set_checksum(from, src.ip(), &mut data, 2);
    IcmpMsg {
        src: from,
        dst: src.ip(),
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
    /// (`icmp_rcv`, `icmpv6_rcv`).
    pub(crate) fn icmp_input(&self, msg: IcmpMsg, iif: u32, back: Back) {
        let raws: Vec<_> = {
            let mut st = self.state();
            st.icmp.raw.retain(|w| w.strong_count() > 0);
            st.icmp.raw.clone()
        };
        for raw in raws.iter().filter_map(Weak::upgrade) {
            raw.raw_input(&msg, iif);
        }
        if msg.data.len() < ICMP_HDR || !checksum_ok(&msg) {
            return;
        }
        let v6 = msg.dst.is_ipv6();
        match (v6, msg.data[0]) {
            (false, ICMP_ECHO) | (true, ICMPV6_ECHO_REQUEST) => self.icmp_echo(&msg, iif, back),
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
    /// `icmp_echo_ignore_broadcasts`.
    fn icmp_echo(&self, msg: &IcmpMsg, iif: u32, back: Back) {
        let src = {
            let st = self.state();
            if msg.dst.is_ipv4()
                && (st.icmp.echo_ignore_all
                    || (msg.dst.is_multicast() && st.icmp.echo_ignore_broadcasts))
            {
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
        data[0] = match msg.dst {
            IpAddr::V4(_) => ICMP_ECHOREPLY,
            IpAddr::V6(_) => ICMPV6_ECHO_REPLY,
        };
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
        back.net.icmp_input(reply, back.iif, me);
    }

    /// `ping_rcv`: an echo reply goes to the ping socket its identifier names.
    fn ping_input(&self, msg: &IcmpMsg, iif: u32) {
        let ident = u16::from_be_bytes([msg.data[4], msg.data[5]]);
        let candidates = self.state().icmp.pings(ident);
        for sock in candidates.iter().filter_map(Weak::upgrade) {
            if sock.ping_match(msg.dst, iif) {
                sock.queue(msg.data.clone(), msg, iif);
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

    /// Sends an ICMP message along `path` (the route a socket looked up).
    fn icmp_deliver(&self, msg: IcmpMsg, path: IpPath) {
        if let IpPath::Local {
            net, iif, back_iif, ..
        } = path
        {
            let back = Back {
                net: self.arc(),
                iif: back_iif,
            };
            net.icmp_input(msg, iif, back);
        }
    }

    /// A UDP datagram from `src` to `dst` that no socket of this namespace takes: the port
    /// unreachable `__udp4_lib_rcv` and `__udp6_lib_rcv` send goes back to the sender's
    /// namespace.
    pub(crate) fn udp_unreachable(
        &self,
        src: SocketAddr,
        dst: SocketAddr,
        payload: &[u8],
        back: Back,
    ) {
        let (type_, code) = match dst {
            SocketAddr::V4(_) => (ICMP_DEST_UNREACH, ICMP_PORT_UNREACH),
            SocketAddr::V6(_) => (ICMPV6_DEST_UNREACH, ICMPV6_PORT_UNREACH),
        };
        let msg = udp_error(type_, code, dst.ip(), src, dst, payload);
        let me = Back {
            net: self.arc(),
            iif: 0,
        };
        back.net.icmp_input(msg, back.iif, me);
    }

    /// `net.ipv4.icmp_echo_ignore_all` (`broadcasts` false) or
    /// `icmp_echo_ignore_broadcasts`.
    pub(crate) fn set_echo_ignore(&self, broadcasts: bool, ignore: bool) {
        let mut st = self.state();
        match broadcasts {
            true => st.icmp.echo_ignore_broadcasts = ignore,
            false => st.icmp.echo_ignore_all = ignore,
        }
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
    queue: VecDeque<Received>,
    queued: usize,
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

/// A ping socket (`SOCK_DGRAM`) or raw ICMP socket (`SOCK_RAW`) of a [`Net`].
pub struct IcmpSocket {
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    raw: bool,
    ino: AtomicU64,
    wq: WaitQueue,
    me: Weak<IcmpSocket>,
    inner: Mutex<Inner>,
}

impl IcmpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain, raw: bool) -> Arc<Self> {
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
            ino: AtomicU64::new(0),
            wq: WaitQueue::new(),
            me: me.clone(),
            inner: Mutex::new(Inner {
                opts: SockOpts::new(kind, domain),
                local: None,
                ident: None,
                addr_locked: false,
                peer: None,
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                errqueue: VecDeque::new(),
                filter: [0; 8],
                // `rawv6_init_sk`: ICMPv6 always has its checksum computed, at offset 2.
                csum_offset: (raw && domain == Domain::Inet6).then_some(2),
                hdrincl: false,
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

    /// `raw_v4_input`/`ipv6_raw_deliver` with `raw_rcv`: a copy of every ICMP message of the
    /// socket's family to its bound address, from its peer, on its device, of a group it
    /// allows, that its filter passes.
    fn raw_input(&self, msg: &IcmpMsg, iif: u32) {
        if msg.dst.is_ipv6() != self.v6() {
            return;
        }
        let g = self.lock();
        let dev = self.device(&g);
        let takes = g.local.is_none_or(|l| l == msg.dst)
            && g.peer.is_none_or(|p| p == msg.src)
            && (dev == 0 || dev == iif)
            && (!msg.dst.is_multicast() || g.opts.mc.allows(msg.dst, iif))
            && msg.data.len() >= ICMP_HDR
            && !filtered(&g.filter, self.v6(), msg.data[0]);
        drop(g);
        if !takes {
            return;
        }
        let data = match (msg.src, msg.dst) {
            (IpAddr::V4(s), IpAddr::V4(d)) => {
                let id = self.net.state().icmp.next_ip_id();
                let h = ipv4_header(s, d, IPPROTO_ICMP as u8, msg.ttl, msg.data.len(), id);
                [&h[..], &msg.data].concat()
            }
            _ => msg.data.clone(),
        };
        self.queue(data, msg, iif);
    }

    /// Queues a message, dropped when the receive buffer is full (`sock_queue_rcv_skb`).
    fn queue(&self, data: Vec<u8>, msg: &IcmpMsg, iif: u32) {
        let mut g = self.lock();
        if g.queued > g.opts.rcvbuf {
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
        let v = match self.domain {
            Domain::Inet => g.opts.int(SOL_IP, IpOption::IP_TTL.0),
            Domain::Inet6 => g.opts.int(SOL_IPV6, Ipv6Option::IPV6_UNICAST_HOPS.0),
        };
        u8::try_from(v)
            .ok()
            .filter(|&t| t > 0)
            .unwrap_or(DEFAULT_TTL)
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
                    let mut data = (r.iif as i32).to_le_bytes().to_vec();
                    data.extend(v4(spec.unwrap_or(r.dst)).octets());
                    data.extend(v4(r.dst).octets());
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
                    let mut data = v6(r.dst).octets().to_vec();
                    data.extend(r.iif.to_le_bytes());
                    out.push(Control {
                        level: SOL_IPV6,
                        kind: Ipv6Option::IPV6_PKTINFO.0,
                        data,
                    });
                }
                if g.opts.flag(SOL_IPV6, Ipv6Option::IPV6_RECVHOPLIMIT.0) {
                    out.push(int(SOL_IPV6, Ipv6Option::IPV6_HOPLIMIT.0, i32::from(r.ttl)));
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

    /// `raw_send_hdrinc`: a raw IPv4 socket with `IP_HDRINCL` sends the IP header it is given:
    /// its source (if any), TTL and protocol, and the ICMP message after it. Another protocol
    /// is not carried. `EINVAL` for a short or malformed header.
    fn strip_header(data: &[u8]) -> NetResult<(Option<IpAddr>, u8, u8, &[u8])> {
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
        match self.domain {
            Domain::Inet => IPPROTO_ICMP,
            Domain::Inet6 => IPPROTO_ICMPV6,
        }
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
        Ok(())
    }

    fn disconnect(&self) -> NetResult<()> {
        let mut g = self.lock();
        g.peer = None;
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
        let (mut data, hdr_src, hdr_ttl) = match self.raw && g.hdrincl {
            true => {
                let (src, ttl, proto, rest) = Self::strip_header(msg.data)?;
                if i32::from(proto) != IPPROTO_ICMP {
                    return Ok(msg.data.len());
                }
                (rest.to_vec(), src, Some(ttl))
            }
            false => (msg.data.to_vec(), None, None),
        };
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
            let ttl = hdr_ttl.or(ctl.ttl).unwrap_or_else(|| self.unicast_ttl(&g));
            return self.send_unicast(g, data, dst, ttl, ident, path, msg.data.len());
        }
        let (mc_oif, mc_src) = g.opts.mc.interface(dst);
        let oif = if oif != 0 { oif } else { mc_oif };
        let route = self.net.mc_route(dst, oif, src_wanted.or(mc_src))?;
        let (ttl, looped) = g.opts.mc.ttl_loop(dst);
        let ttl = hdr_ttl.or(ctl.ttl).unwrap_or(ttl);
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        self.finish(&g, &mut data, route.src, dst, ident)?;
        drop(g);
        let message = IcmpMsg {
            src: route.src,
            dst,
            ttl,
            data,
        };
        for (net, iif) in self.net.mc_receivers(dst, route.oif, looped, ttl) {
            let back = Back {
                net: self.net.clone(),
                iif: route.oif,
            };
            net.icmp_input(message.clone(), iif, back);
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
                        if let Some(r) = self.backend_reply(&g, tmp, from) {
                            g.queued += r.data.len() + OVERHEAD;
                            g.queue.push_back(r);
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
        match (level, self.raw, self.domain) {
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
            (SOL_RAW, true, Domain::Inet6) => {
                let val = read_int(value)?;
                return match name {
                    IPV6_CHECKSUM if val > 0 && val & 1 != 0 => Err(Errno::EINVAL),
                    IPV6_CHECKSUM => {
                        g.csum_offset = usize::try_from(val).ok();
                        Ok(())
                    }
                    n if n == Ipv6Option::IPV6_HDRINCL.0 => Ok(()),
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
            // A raw IPv6 socket has no IPv4 options; ICMPv6's checksum is not the IPv6
            // level's to change.
            (SOL_IP, true, Domain::Inet6) => return Err(Errno::ENOPROTOOPT),
            (SOL_IPV6, true, Domain::Inet6) if name == IPV6_CHECKSUM => return Err(Errno::EINVAL),
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
        match (level, self.raw, self.domain) {
            (SOL_RAW, true, Domain::Inet) if name == ICMP_FILTER => {
                return Ok(write_bytes(out, &g.filter[0].to_le_bytes()));
            }
            (SOL_RAW, true, Domain::Inet6) if name == IPV6_CHECKSUM => {
                let v = g.csum_offset.map_or(-1, |o| o as i32);
                return Ok(write_int(out, v));
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

    /// A unicast send along `path`, from its source.
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
    ) -> NetResult<usize> {
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        let src = path.src();
        self.finish(&g, &mut data, src, dst, ident)?;
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
        self.net.icmp_deliver(msg, path);
        Ok(len)
    }
}
