//! Control messages of IP sends: `udp_cmsg_send`, `ip_cmsg_send` and `ip6_datagram_send_ctl`.
//! The kernel's socket layer has checked and removed the `SOL_SOCKET` ones
//! (`__sock_cmsg_send`), so what reaches a socket is protocol-level.
//!
//! Every message is validated as Linux validates it, in order, and the first bad one fails the
//! send. Of what they ask for, the source address and interface of `IP_PKTINFO` and
//! `IPV6_PKTINFO` and the segment size of `UDP_SEGMENT` change what a guest can observe and are
//! applied; hop limits, traffic classes and IP options describe packets, which no destination
//! ever sees here, so they are only checked.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use wasixv2_abi::{Errno, IpOption, Ipv6Option, SocketLevel};

use crate::{Control, Domain, NetResult};

const SOL_IP: i32 = SocketLevel::SOL_IP.0;
const SOL_IPV6: i32 = SocketLevel::SOL_IPV6.0;
/// `SOL_UDP`, which is `IPPROTO_UDP`.
const SOL_UDP: i32 = 17;
/// `UDP_SEGMENT`: the generic segmentation offload size of a send.
const UDP_SEGMENT: i32 = 103;
/// `UDP_MAX_SEGMENTS`: the most segments one send may make.
const UDP_MAX_SEGMENTS: usize = 1 << 7;
/// `IP_PROTOCOL`.
const IP_PROTOCOL: i32 = 52;
/// The RFC 2292 numbers `ip6_datagram_send_ctl` still takes.
const IPV6_2292PKTINFO: i32 = 2;
const IPV6_2292HOPOPTS: i32 = 3;
const IPV6_2292DSTOPTS: i32 = 4;
const IPV6_2292RTHDR: i32 = 5;
const IPV6_2292HOPLIMIT: i32 = 8;
const IPV6_FLOWINFO: i32 = 11;
/// `IPV6_SRCRT_TYPE_2`, the only routing header a send may carry.
const IPV6_SRCRT_TYPE_2: u8 = 2;
/// `sizeof(struct in_pktinfo)` and `sizeof(struct in6_pktinfo)`.
const IN_PKTINFO: usize = 12;
const IN6_PKTINFO: usize = 20;

/// What a send's control messages ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SendCtl {
    /// `ipc->addr` / `fl6->saddr`: the source address, if one was given.
    pub src: Option<IpAddr>,
    /// `ipc->oif` / `fl6->flowi6_oif`: the outgoing interface, 0 for any.
    pub oif: u32,
    /// `UDP_SEGMENT`'s size: the payload goes out as datagrams of this size.
    pub gso_size: Option<usize>,
    /// `IP_TTL` / `IPV6_HOPLIMIT`: the datagram's hop limit, if given (`ipc->ttl`,
    /// `ipc6->hlimit`).
    pub ttl: Option<u8>,
}

fn int(c: &Control) -> NetResult<i32> {
    let bytes: [u8; 4] = c.data.as_slice().try_into().map_err(|_| Errno::EINVAL)?;
    Ok(i32::from_le_bytes(bytes))
}

/// `udp_cmsg_send`, then the IP family's checks if anything is left for them: an IPv4
/// destination (also one an IPv6 socket reaches through a mapped address) is
/// `ip_cmsg_send`'s, an IPv6 one `ip6_datagram_send_ctl`'s. `net_raw` is `CAP_NET_RAW`.
pub(crate) fn udp(
    controls: &[Control],
    domain: Domain,
    v4_dst: bool,
    net_raw: bool,
) -> NetResult<SendCtl> {
    let mut ctl = SendCtl::default();
    let mut need_ip = false;
    for c in controls {
        if c.level != SOL_UDP {
            need_ip = true;
            continue;
        }
        match c.kind {
            UDP_SEGMENT => {
                let bytes: [u8; 2] = c.data.as_slice().try_into().map_err(|_| Errno::EINVAL)?;
                ctl.gso_size = Some(u16::from_le_bytes(bytes) as usize).filter(|&n| n > 0);
            }
            _ => return Err(Errno::EINVAL),
        }
    }
    if need_ip {
        let ip = match v4_dst {
            true => ip(controls, domain == Domain::Inet6)?,
            false => ip6(controls, net_raw)?,
        };
        ctl.src = ip.src;
        ctl.oif = ip.oif;
        ctl.ttl = ip.ttl;
    }
    Ok(ctl)
}

/// The IP family's checks of a send without a transport level of its own (ping and raw
/// sockets): `ip_cmsg_send` for an IPv4 destination, `ip6_datagram_send_ctl` for IPv6.
pub(crate) fn inet(
    controls: &[Control],
    domain: Domain,
    v4_dst: bool,
    net_raw: bool,
) -> NetResult<SendCtl> {
    match v4_dst {
        true => ip(controls, domain == Domain::Inet6),
        false => ip6(controls, net_raw),
    }
}

/// `udp_send_skb`'s segmentation checks: at most `UDP_MAX_SEGMENTS` segments.
pub(crate) fn check_segments(len: usize, gso_size: usize) -> NetResult<()> {
    match len > gso_size * UDP_MAX_SEGMENTS {
        true => Err(Errno::EINVAL),
        false => Ok(()),
    }
}

/// `ip_cmsg_send`. `allow_ipv6` for an IPv6 socket sending to a mapped address, which may
/// give `IPV6_PKTINFO` with a mapped source.
fn ip(controls: &[Control], allow_ipv6: bool) -> NetResult<SendCtl> {
    let mut ctl = SendCtl::default();
    for c in controls {
        if allow_ipv6 && c.level == SOL_IPV6 && c.kind == Ipv6Option::IPV6_PKTINFO.0 {
            if c.data.len() < IN6_PKTINFO {
                return Err(Errno::EINVAL);
            }
            let addr = Ipv6Addr::from(<[u8; 16]>::try_from(&c.data[..16]).expect("16 bytes"));
            let src = addr.to_ipv4_mapped().ok_or(Errno::EINVAL)?;
            let index = u32::from_le_bytes(c.data[16..20].try_into().expect("4 bytes"));
            if index != 0 {
                ctl.oif = index;
            }
            ctl.src = Some(src.into()).filter(|ip: &IpAddr| !ip.is_unspecified());
            continue;
        }
        if c.level != SOL_IP {
            continue;
        }
        match c.kind {
            // `ip_options_get` takes at most 40 bytes of options, which are never sent.
            k if k == IpOption::IP_RETOPTS.0 => {}
            k if k == IpOption::IP_PKTINFO.0 => {
                if c.data.len() != IN_PKTINFO {
                    return Err(Errno::EINVAL);
                }
                let index = u32::from_le_bytes(c.data[..4].try_into().expect("4 bytes"));
                if index != 0 {
                    ctl.oif = index;
                }
                let spec_dst = Ipv4Addr::from(<[u8; 4]>::try_from(&c.data[4..8]).expect("4"));
                ctl.src = Some(spec_dst.into()).filter(|ip: &IpAddr| !ip.is_unspecified());
            }
            k if k == IpOption::IP_TTL.0 => {
                let ttl = int(c)?;
                if !(1..=255).contains(&ttl) {
                    return Err(Errno::EINVAL);
                }
                ctl.ttl = Some(ttl as u8);
            }
            k if k == IpOption::IP_TOS.0 => {
                let tos = match c.data.len() {
                    4 => int(c)?,
                    1 => c.data[0] as i32,
                    _ => return Err(Errno::EINVAL),
                };
                if !(0..=255).contains(&tos) {
                    return Err(Errno::EINVAL);
                }
            }
            IP_PROTOCOL => {
                if !(1..=255).contains(&int(c)?) {
                    return Err(Errno::EINVAL);
                }
            }
            _ => return Err(Errno::EINVAL),
        }
    }
    Ok(ctl)
}

/// The length an IPv6 extension header says it has (`ipv6_optlen`), checked against the
/// control message: `EINVAL` if the message is shorter than the header or the header's length.
fn ext_header_len(c: &Control) -> NetResult<usize> {
    let hdrlen = *c.data.get(1).ok_or(Errno::EINVAL)?;
    let len = (hdrlen as usize + 1) << 3;
    match c.data.len() < len {
        true => Err(Errno::EINVAL),
        false => Ok(len),
    }
}

/// `ip6_datagram_send_ctl`.
fn ip6(controls: &[Control], net_raw: bool) -> NetResult<SendCtl> {
    let mut ctl = SendCtl::default();
    let mut hopopts = false;
    for c in controls {
        if c.level != SOL_IPV6 {
            continue;
        }
        match c.kind {
            k if k == Ipv6Option::IPV6_PKTINFO.0 || k == IPV6_2292PKTINFO => {
                if c.data.len() < IN6_PKTINFO {
                    return Err(Errno::EINVAL);
                }
                let addr = Ipv6Addr::from(<[u8; 16]>::try_from(&c.data[..16]).expect("16 bytes"));
                let index = u32::from_le_bytes(c.data[16..20].try_into().expect("4 bytes"));
                if index != 0 {
                    ctl.oif = index;
                }
                // A link-local source needs an interface to be unambiguous.
                if ctl.oif == 0 && addr.is_unicast_link_local() {
                    return Err(Errno::EINVAL);
                }
                if !addr.is_unspecified() {
                    ctl.src = Some(addr.into());
                }
            }
            IPV6_FLOWINFO => {
                if c.data.len() < 4 {
                    return Err(Errno::EINVAL);
                }
            }
            k if k == Ipv6Option::IPV6_HOPOPTS.0 || k == IPV6_2292HOPOPTS => {
                if hopopts || c.data.len() < 2 {
                    return Err(Errno::EINVAL);
                }
                ext_header_len(c)?;
                if !net_raw {
                    return Err(Errno::EPERM);
                }
                hopopts = true;
            }
            k if k == IPV6_2292DSTOPTS
                || k == Ipv6Option::IPV6_DSTOPTS.0
                || k == Ipv6Option::IPV6_RTHDRDSTOPTS.0 =>
            {
                if c.data.len() < 2 {
                    return Err(Errno::EINVAL);
                }
                ext_header_len(c)?;
                if !net_raw {
                    return Err(Errno::EPERM);
                }
            }
            k if k == Ipv6Option::IPV6_RTHDR.0 || k == IPV6_2292RTHDR => {
                // `struct ipv6_rt_hdr`: next header, length, type, segments left.
                let [_, hdrlen, kind, segments_left] = match c.data.get(..4) {
                    Some(&[a, b, k, s]) => [a, b, k, s],
                    _ => return Err(Errno::EINVAL),
                };
                if kind != IPV6_SRCRT_TYPE_2 || hdrlen != 2 || segments_left != 1 {
                    return Err(Errno::EINVAL);
                }
                ext_header_len(c)?;
                if (hdrlen >> 1) != segments_left {
                    return Err(Errno::EINVAL);
                }
            }
            k if k == Ipv6Option::IPV6_HOPLIMIT.0 || k == IPV6_2292HOPLIMIT => {
                let hops = int(c)?;
                if !(-1..=255).contains(&hops) {
                    return Err(Errno::EINVAL);
                }
                ctl.ttl = u8::try_from(hops).ok();
            }
            k if k == Ipv6Option::IPV6_TCLASS.0 => {
                if !(-1..=255).contains(&int(c)?) {
                    return Err(Errno::EINVAL);
                }
            }
            k if k == Ipv6Option::IPV6_DONTFRAG.0 => {
                if !(0..=1).contains(&int(c)?) {
                    return Err(Errno::EINVAL);
                }
            }
            _ => return Err(Errno::EINVAL),
        }
    }
    Ok(ctl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(level: i32, kind: i32, data: &[u8]) -> Control {
        Control {
            level,
            kind,
            data: data.to_vec(),
        }
    }

    fn ttl(v: i32) -> Control {
        c(SOL_IP, IpOption::IP_TTL.0, &v.to_le_bytes())
    }

    #[test]
    fn ip_messages_are_validated_in_order() {
        let udp4 = |cs: &[Control]| udp(cs, Domain::Inet, true, false);
        let ttl64 = SendCtl {
            ttl: Some(64),
            ..SendCtl::default()
        };
        assert_eq!(udp4(&[ttl(64)]), Ok(ttl64));
        assert_eq!(udp4(&[ttl(0)]), Err(Errno::EINVAL));
        assert_eq!(
            udp4(&[c(SOL_IP, IpOption::IP_TTL.0, &[64])]),
            Err(Errno::EINVAL)
        );
        assert_eq!(udp4(&[c(SOL_IP, 9999, &[])]), Err(Errno::EINVAL));
        // A one-byte IP_TOS is accepted, as is an int; 256 is not.
        assert!(udp4(&[c(SOL_IP, IpOption::IP_TOS.0, &[0x10])]).is_ok());
        assert_eq!(
            udp4(&[c(SOL_IP, IpOption::IP_TOS.0, &256i32.to_le_bytes())]),
            Err(Errno::EINVAL)
        );
        // Other levels are not IPv4's to judge.
        assert!(udp4(&[c(SOL_IPV6, 9999, &[])]).is_ok());
        assert!(udp4(&[c(SOL_UDP, 9999, &[])]).is_err());
    }

    #[test]
    fn pktinfo_gives_the_source_and_interface() {
        let mut info = 2u32.to_le_bytes().to_vec();
        info.extend([127, 0, 0, 2, 0, 0, 0, 0]);
        let ctl = udp(
            &[c(SOL_IP, IpOption::IP_PKTINFO.0, &info)],
            Domain::Inet,
            true,
            false,
        )
        .unwrap();
        assert_eq!(ctl.src, Some(Ipv4Addr::new(127, 0, 0, 2).into()));
        assert_eq!(ctl.oif, 2);
        assert_eq!(
            udp(
                &[c(SOL_IP, IpOption::IP_PKTINFO.0, &info[..8])],
                Domain::Inet,
                true,
                false
            ),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn ipv6_messages_and_option_headers() {
        let udp6 = |cs: &[Control], raw| udp(cs, Domain::Inet6, false, raw);
        let hop = |v: i32| c(SOL_IPV6, Ipv6Option::IPV6_HOPLIMIT.0, &v.to_le_bytes());
        assert!(udp6(&[hop(-1)], false).is_ok());
        assert_eq!(udp6(&[hop(256)], false), Err(Errno::EINVAL));
        // An IPv4 option on an IPv6 destination is ignored.
        assert!(udp6(&[ttl(0)], false).is_ok());
        let hopopts = c(
            SOL_IPV6,
            Ipv6Option::IPV6_HOPOPTS.0,
            &[0, 0, 1, 4, 0, 0, 0, 0],
        );
        assert_eq!(
            udp6(std::slice::from_ref(&hopopts), false),
            Err(Errno::EPERM)
        );
        assert!(udp6(std::slice::from_ref(&hopopts), true).is_ok());
        assert_eq!(udp6(&[hopopts.clone(), hopopts], true), Err(Errno::EINVAL));
        // A header claiming more than the message holds.
        let short = c(SOL_IPV6, Ipv6Option::IPV6_DSTOPTS.0, &[0, 1, 0, 0]);
        assert_eq!(udp6(&[short], true), Err(Errno::EINVAL));
    }

    #[test]
    fn udp_segment_size() {
        let seg = |n: u16| c(SOL_UDP, UDP_SEGMENT, &n.to_le_bytes());
        let ctl = udp(&[seg(1000)], Domain::Inet, true, false).unwrap();
        assert_eq!(ctl.gso_size, Some(1000));
        assert_eq!(
            udp(&[c(SOL_UDP, UDP_SEGMENT, &[1])], Domain::Inet, true, false),
            Err(Errno::EINVAL)
        );
        assert_eq!(check_segments(128 * 1000, 1000), Ok(()));
        assert_eq!(check_segments(128 * 1000 + 1, 1000), Err(Errno::EINVAL));
    }
}
