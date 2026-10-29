//! Raw sockets of every protocol (`net/ipv4/raw.c`, `net/ipv6/raw.c`), as far as a network of
//! messages has packets for them. A raw socket is an [`IcmpSocket`](crate::IcmpSocket) of its
//! protocol, and every packet that arrives in its namespace is offered to the raw sockets of
//! its family and protocol first (`raw_v4_input`, `ipv6_raw_deliver`):
//!
//! - UDP datagrams, as UDP sockets of the machine send them to the namespace (unicast and to
//!   groups), header and checksum made up;
//! - ICMP messages (see `icmp.rs`);
//! - what raw sockets send: a packet of the socket's protocol, or with `IP_HDRINCL` (always on
//!   for `IPPROTO_RAW`, and `IPV6_HDRINCL` on IPv6) the header the sender wrote, which names
//!   the protocol, source and TTL. At the destination it is handed on as
//!   `ip_local_deliver_finish` hands it: ICMP to the kernel's ICMP, UDP to the socket of its
//!   port (or a port unreachable), and a protocol neither a handler nor a raw socket takes
//!   answered with a protocol unreachable (ICMPv6: an unknown next header).
//!
//! TCP streams are in memory and make no segments, so a raw TCP socket only hears what raw
//! sockets send; a TCP segment a raw socket sends reaches no TCP socket. Nothing raw leaves
//! for the host.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Weak,
    time::Duration,
};

use crate::{
    Socket,
    bpf::Arrival,
    icmp::{
        Back, IPPROTO_ICMP, IPPROTO_ICMPV6, IPPROTO_UDP, IcmpMsg, UDP_HDR, quote_error,
        set_checksum, udp_checksum_ok,
    },
    net::{Net, udp_receiver},
};

/// `IPPROTO_TCP`, `IPPROTO_RAW` and `IPPROTO_MH`.
pub(crate) const IPPROTO_TCP: u8 = 6;
pub(crate) const IPPROTO_RAW: u8 = 255;
pub(crate) const IPPROTO_MH: u8 = 135;
/// `IPPROTO_MAX`: protocols of raw sockets are below it.
pub(crate) const IPPROTO_MAX: i32 = 263;
const ICMP_DEST_UNREACH: u8 = 3;
const ICMP_PROT_UNREACH: u8 = 2;
const ICMPV6_PARAMPROB: u8 = 4;
const ICMPV6_UNK_NEXTHDR: u8 = 1;
/// Where an IPv6 header holds its next header, which an unknown one's parameter problem
/// points at.
const IPV6_NEXTHDR_OFFSET: u32 = 6;

/// An IP packet on its way: what its header says, and what follows it.
#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub src: IpAddr,
    pub dst: IpAddr,
    /// The IPv4 TTL or IPv6 hop limit it arrives with.
    pub ttl: u8,
    pub proto: u8,
    pub data: Vec<u8>,
}

impl Net {
    /// Whether a raw socket of `proto` is open in the namespace, so a copy is worth making.
    pub(crate) fn has_raw(&self, proto: u8) -> bool {
        self.raw_sockets()
            .iter()
            .filter_map(Weak::upgrade)
            .any(|s| s.protocol() == i32::from(proto))
    }

    fn raw_sockets(&self) -> Vec<Weak<crate::IcmpSocket>> {
        let mut st = self.state();
        st.icmp.raw.retain(|w| w.strong_count() > 0);
        st.icmp.raw.clone()
    }

    /// `raw_v4_input`, `ipv6_raw_deliver`: offers `p`, arriving on `iif`, to every raw socket
    /// of its family and protocol. Returns whether one took it.
    pub(crate) fn raw_input(&self, p: &Packet, iif: u32, pkttype: u8) -> bool {
        let mut taken = false;
        for sock in self.raw_sockets().iter().filter_map(Weak::upgrade) {
            taken |= sock.raw_input(p, iif, pkttype);
        }
        taken
    }

    /// A UDP datagram arrives from a socket of the machine: the raw UDP sockets get a copy.
    pub(crate) fn raw_udp_copy(
        &self,
        from: SocketAddr,
        dst: SocketAddr,
        at: &Arrival,
        data: &[u8],
    ) {
        if !self.has_raw(IPPROTO_UDP) {
            return;
        }
        let p = Packet {
            src: from.ip(),
            dst: dst.ip(),
            ttl: at.ttl,
            proto: IPPROTO_UDP,
            data: crate::icmp::udp_datagram(from, dst, data),
        };
        self.raw_input(&p, at.iif, at.pkttype);
    }

    /// `ip_local_deliver_finish`, `ip6_input_finish` for a packet a raw socket sent, arriving
    /// on `iif` from the namespace `back` names, at `now`.
    pub(crate) fn ip_input(&self, p: Packet, iif: u32, back: Back, now: Duration) {
        let icmp = match p.dst {
            IpAddr::V4(_) => IPPROTO_ICMP,
            IpAddr::V6(_) => IPPROTO_ICMPV6,
        } as u8;
        if p.proto == icmp {
            let msg = IcmpMsg {
                src: p.src,
                dst: p.dst,
                ttl: p.ttl,
                data: p.data,
            };
            return self.icmp_input(msg, iif, back, now);
        }
        let pkttype = self.pkttype(p.dst, &back);
        let raw = self.raw_input(&p, iif, pkttype);
        match p.proto {
            IPPROTO_UDP => self.udp_input(&p, iif, pkttype, back, now),
            // `tcp_v4_rcv` would answer with a reset, which only a TCP socket hears.
            IPPROTO_TCP => {}
            _ if !raw && !p.dst.is_multicast() => self.proto_unreachable(&p, iif, back, now),
            _ => {}
        }
    }

    /// `udp_rcv`, `udpv6_rcv`: a whole datagram with a good checksum goes to the socket of
    /// its port, or to a group's members; nobody there is a port unreachable.
    fn udp_input(&self, p: &Packet, iif: u32, pkttype: u8, back: Back, now: Duration) {
        if p.data.len() < UDP_HDR {
            return;
        }
        let word = |at: usize| u16::from_be_bytes([p.data[at], p.data[at + 1]]);
        let len = usize::from(word(4));
        if len < UDP_HDR || len > p.data.len() || !udp_checksum_ok(p.src, p.dst, &p.data[..len]) {
            return;
        }
        let payload = &p.data[UDP_HDR..len];
        let from = SocketAddr::new(p.src, word(0));
        let dst = SocketAddr::new(p.dst, word(2));
        let at = Arrival {
            src: p.src,
            dst: p.dst,
            ttl: p.ttl,
            iif,
            pkttype,
        };
        if p.dst.is_multicast() {
            return self.udp_mcast_deliver(dst, from, &at, payload);
        }
        let to = udp_receiver(&self.state(), p.src, from, dst).and_then(|w| w.upgrade());
        match to {
            Some(sock) => sock.deliver(payload, from, &at),
            None => self.udp_unreachable(from, dst, payload, iif, back, now),
        }
    }

    /// `icmp_send(ICMP_DEST_UNREACH, ICMP_PROT_UNREACH)` and
    /// `icmpv6_send(ICMPV6_PARAMPROB, ICMPV6_UNK_NEXTHDR)` back to the sender of a packet
    /// nothing here takes, as the rate limits allow.
    fn proto_unreachable(&self, p: &Packet, iif: u32, back: Back, now: Duration) {
        let mut msg = match p.dst {
            IpAddr::V4(_) => quote_error(
                ICMP_DEST_UNREACH,
                ICMP_PROT_UNREACH,
                p.dst,
                p.src,
                p.dst,
                p.proto,
                &p.data,
            ),
            IpAddr::V6(_) => {
                let mut msg = quote_error(
                    ICMPV6_PARAMPROB,
                    ICMPV6_UNK_NEXTHDR,
                    p.dst,
                    p.src,
                    p.dst,
                    p.proto,
                    &p.data,
                );
                msg.data[4..8].copy_from_slice(&IPV6_NEXTHDR_OFFSET.to_be_bytes());
                msg
            }
        };
        set_checksum(msg.src, msg.dst, &mut msg.data, 2);
        if !self.error_allowed(
            &msg,
            iif == crate::iface::LOOPBACK_INDEX,
            self.is(&back.net),
            now,
        ) {
            return;
        }
        let me = Back {
            net: self.arc(),
            iif: 0,
        };
        back.net.icmp_input(msg, back.iif, me, now);
    }
}
