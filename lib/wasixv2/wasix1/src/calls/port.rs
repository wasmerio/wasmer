//! WASIX v1 network queries and configuration through `NETLINK_ROUTE`.
//! Bridge/DHCP require provider controls that this transport does not expose.

use crate::abi::Pointer;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use wasixv2_abi::{AddressFamily, GuestPtr, GuestPtrMut, MsgFlags, SocketFlags, SocketType};
use wasixv2_syscalls::{Control, Syscalls, UserSliceMut};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno, GuestLayout,
        proc::{OptionTag, OptionTimestamp},
        sock::{Addr, AddrCidr, Addressfamily, HardwareAddress, Route},
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

const NETLINK_ROUTE: i32 = 0;
const NLMSG_HDR: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
const NLM_F_ACK: u16 = 4;
const NLM_F_CREATE: u16 = 0x400;
const NLM_F_REPLACE: u16 = 0x100;
const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
const RTM_DELADDR: u16 = 21;
const RTM_NEWROUTE: u16 = 24;
const RTM_DELROUTE: u16 = 25;
const RTM_GETROUTE: u16 = 26;
const RTMSG: usize = 12;
const RTA_DST: u16 = 1;
const RTA_GATEWAY: u16 = 5;
const RTA_OIF: u16 = 4;
const IFA_LOCAL: u16 = 2;
const IFLA_ADDRESS: u16 = 1;
const IFA_ADDRESS: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
/// `sizeof(struct ifinfomsg)` and `sizeof(struct ifaddrmsg)`.
const IFINFOMSG: usize = 16;
const IFADDRMSG: usize = 8;

/// A dump request for `kind` with an all-zero family header of `body` bytes.
fn request(kind: u16, body: usize) -> Vec<u8> {
    let len = NLMSG_HDR + body;
    let mut msg = Vec::with_capacity(len);
    msg.extend_from_slice(&(len as u32).to_le_bytes());
    msg.extend_from_slice(&kind.to_le_bytes());
    msg.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_le_bytes());
    msg.extend_from_slice(&1u32.to_le_bytes()); // seq
    msg.extend_from_slice(&0u32.to_le_bytes()); // pid: the kernel
    msg.resize(len, 0);
    msg
}

/// Appends the messages in `data` to `out` as (type, payload); whether `NLMSG_DONE` ended the
/// dump. An `NLMSG_ERROR` ends it with its errno.
fn messages(data: &[u8], out: &mut Vec<(u16, Vec<u8>)>) -> Result<bool, wasixv2_abi::Errno> {
    let mut at = 0;
    while data.len().saturating_sub(at) >= NLMSG_HDR {
        let h = &data[at..];
        let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
        if len < NLMSG_HDR || len > h.len() {
            break;
        }
        let kind = u16::from_le_bytes([h[4], h[5]]);
        let payload = &h[NLMSG_HDR..len];
        match kind {
            NLMSG_DONE => return Ok(true),
            NLMSG_ERROR => {
                let errno = payload
                    .get(..4)
                    .map_or(0, |e| i32::from_le_bytes(e.try_into().unwrap()));
                if errno != 0 {
                    return Err(wasixv2_abi::Errno(-errno));
                }
                return Ok(true);
            }
            _ => out.push((kind, payload.to_vec())),
        }
        at += len.next_multiple_of(4);
    }
    Ok(false)
}

/// The attributes after a family header of `header` bytes, as (type, data).
fn attributes(payload: &[u8], header: usize) -> Vec<(u16, &[u8])> {
    let mut attrs = Vec::new();
    let mut at = header;
    while payload.len().saturating_sub(at) >= 4 {
        let len = u16::from_le_bytes([payload[at], payload[at + 1]]) as usize;
        let kind = u16::from_le_bytes([payload[at + 2], payload[at + 3]]);
        if len < 4 || at + len > payload.len() {
            break;
        }
        attrs.push((kind, &payload[at + 4..at + len]));
        at += len.next_multiple_of(4);
    }
    attrs
}

/// Exchanges one request over a fresh netlink socket.
async fn exchange<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    request: &[u8],
) -> V1Result<Vec<(u16, Vec<u8>)>> {
    let caller = frame.caller();
    let ty = SocketType::SOCK_DGRAM.0 | SocketFlags::SOCK_CLOEXEC.0 as i32;
    let fd = frame
        .syscalls
        .socket(&caller, AddressFamily::AF_NETLINK, ty, NETLINK_ROUTE)
        .await?;
    let result = async {
        let msg = frame.mem.alloc_bytes(request)?;
        frame
            .syscalls
            .sendto(&caller, fd, msg, MsgFlags(0), GuestPtr::NULL, 0)
            .await?;
        let mut out = Vec::new();
        loop {
            // net/netlink/af_netlink.c: netlink_recvmsg supports MSG_PEEK|MSG_TRUNC.
            // Size the whole datagram before consuming it, including a dump's DONE.
            let room = frame
                .syscalls
                .recvfrom(
                    &caller,
                    fd,
                    UserSliceMut::new(0, 0),
                    MsgFlags::MSG_PEEK | MsgFlags::MSG_TRUNC,
                    GuestPtrMut::NULL,
                    GuestPtrMut::NULL,
                )
                .await?;
            let buf = frame.mem.alloc_zeroed(room)?;
            let n = frame
                .syscalls
                .recvfrom(
                    &caller,
                    fd,
                    UserSliceMut::new(buf.addr, buf.len),
                    MsgFlags(0),
                    GuestPtrMut::NULL,
                    GuestPtrMut::NULL,
                )
                .await?;
            let data = frame.mem.take_bytes(buf, n as usize);
            if messages(&data, &mut out)? || n == 0 {
                return Ok(out);
            }
        }
    }
    .await;
    // Closing never blocks, so it is safe after an interrupted receive.
    let _ = frame.syscalls.close(&caller, fd).await;
    result
}

async fn dump<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    kind: u16,
    body: usize,
) -> V1Result<Vec<(u16, Vec<u8>)>> {
    exchange(frame, &request(kind, body)).await
}

/// Mutations ask for an ACK: a successful write only means the request was queued.
async fn change<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    kind: u16,
    flags: u16,
    payload: &[u8],
) -> V1Result<()> {
    let mut msg = request(kind, 0);
    msg[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_ACK | flags).to_le_bytes());
    msg.extend_from_slice(payload);
    let len = msg.len() as u32;
    msg[..4].copy_from_slice(&len.to_le_bytes());
    exchange(frame, &msg).await?;
    Ok(())
}

fn rta(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
    out.extend_from_slice(&((4 + data.len()) as u16).to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

fn ip(family: u8, bytes: &[u8]) -> Option<IpAddr> {
    match i32::from(family) {
        f if f == AddressFamily::AF_INET.0 => {
            Some(Ipv4Addr::from(<[u8; 4]>::try_from(bytes.get(..4)?).ok()?).into())
        }
        f if f == AddressFamily::AF_INET6.0 => {
            Some(Ipv6Addr::from(<[u8; 16]>::try_from(bytes.get(..16)?).ok()?).into())
        }
        _ => None,
    }
}

fn v1_ip(tag: Addressfamily, bytes: &[u8]) -> V1Result<IpAddr> {
    let family = match tag {
        Addressfamily::INET4 => AddressFamily::AF_INET.0,
        Addressfamily::INET6 => AddressFamily::AF_INET6.0,
        _ => return shim::fail(Errno::INVAL),
    };
    match ip(family as u8, bytes) {
        Some(ip) => Ok(ip),
        None => shim::fail(Errno::INVAL),
    }
}

fn family(addr: IpAddr) -> u8 {
    if addr.is_ipv4() {
        AddressFamily::AF_INET.0 as u8
    } else {
        AddressFamily::AF_INET6.0 as u8
    }
}

fn octets(addr: IpAddr) -> Vec<u8> {
    match addr {
        IpAddr::V4(a) => a.octets().to_vec(),
        IpAddr::V6(a) => a.octets().to_vec(),
    }
}

fn cidr(addr: &AddrCidr) -> V1Result<(IpAddr, u8)> {
    let ip = v1_ip(addr.tag, &addr.payload)?;
    let prefix = addr.payload[if ip.is_ipv4() { 4 } else { 24 }];
    if prefix > if ip.is_ipv4() { 32 } else { 128 } {
        return shim::fail(Errno::INVAL);
    }
    Ok((ip, prefix))
}

fn link(msgs: &[(u16, Vec<u8>)]) -> Option<u32> {
    let mut fallback = None;
    for (kind, p) in msgs {
        if *kind != RTM_NEWLINK || p.len() < IFINFOMSG {
            continue;
        }
        let index = u32::from_le_bytes(p[4..8].try_into().unwrap());
        if u16::from_le_bytes(p[2..4].try_into().unwrap()) != ARPHRD_LOOPBACK {
            return Some(index);
        }
        fallback.get_or_insert(index);
    }
    fallback
}

/// Keep the interface index and local address: IPv4 IFA_ADDRESS may describe a peer.
fn address_ip(payload: &[u8]) -> Option<IpAddr> {
    let attrs = attributes(payload, IFADDRMSG);
    let bytes = attrs
        .iter()
        .find(|(k, _)| *k == IFA_LOCAL)
        .or_else(|| attrs.iter().find(|(k, _)| *k == IFA_ADDRESS))?
        .1;
    ip(*payload.first()?, bytes)
}

/// Every address of every interface, with its prefix length.
fn addresses(msgs: &[(u16, Vec<u8>)]) -> Vec<AddrCidr> {
    let mut out = Vec::new();
    for (kind, payload) in msgs {
        if *kind != RTM_NEWADDR || payload.len() < IFADDRMSG {
            continue;
        }
        if let Some(ip) = address_ip(payload) {
            out.push(AddrCidr::new(ip, payload[1]));
        }
    }
    out
}

/// The hardware address of the first non-loopback interface (loopback's if it is the only one).
fn mac(msgs: &[(u16, Vec<u8>)]) -> Option<[u8; 6]> {
    let mut fallback = None;
    for (kind, payload) in msgs {
        if *kind != RTM_NEWLINK || payload.len() < IFINFOMSG {
            continue;
        }
        let loopback = u16::from_le_bytes([payload[2], payload[3]]) == ARPHRD_LOOPBACK;
        let Some(mac) = attributes(payload, IFINFOMSG)
            .into_iter()
            .find(|(attr, data)| *attr == IFLA_ADDRESS && data.len() == 6)
            .map(|(_, data)| <[u8; 6]>::try_from(data).unwrap())
        else {
            continue;
        };
        if !loopback {
            return Some(mac);
        }
        fallback.get_or_insert(mac);
    }
    fallback
}

/// `port_addr_list(addrs, naddrs)`: `*naddrs` holds the room on entry and the count on
/// return; `OVERFLOW` (with the count written) when they do not fit.
pub(crate) async fn port_addr_list<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addrs: Pointer,
    naddrs: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let room = frame.read_size(naddrs).await?;
        let list = addresses(&dump(frame, RTM_GETADDR, IFADDRMSG).await?);
        frame.write_size(naddrs, list.len() as u64).await?;
        if list.len() as u64 > room {
            return shim::fail(Errno::OVERFLOW);
        }
        frame
            .check_range(
                addrs,
                room.checked_mul(AddrCidr::SIZE as u64)
                    .ok_or(Errno::OVERFLOW)?,
            )
            .await?;
        for (i, addr) in list.iter().enumerate() {
            let at = addrs.offset((i * AddrCidr::SIZE) as u64)?.narrow()?;
            frame.write(at, addr).await?;
        }
        Ok(())
    })
    .await
}

pub(crate) async fn port_mac<S: Syscalls + Control + 'static>(
    env: Env<S>,
    ro_mac: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let Some(octets) = mac(&dump(frame, RTM_GETLINK, IFINFOMSG).await?) else {
            return shim::fail(Errno::NODEV);
        };
        frame
            .write(ro_mac.narrow()?, &HardwareAddress { octets })
            .await
    })
    .await
}

fn routes(msgs: &[(u16, Vec<u8>)]) -> Vec<Route> {
    msgs.iter()
        .filter_map(|(kind, p)| {
            if *kind != RTM_NEWROUTE || p.len() < RTMSG {
                return None;
            }
            let any = ip(p[0], &[0; 16])?;
            let attrs = attributes(p, RTMSG);
            let addr = |key| {
                attrs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .and_then(|(_, b)| ip(p[0], b))
                    .unwrap_or(any)
            };
            let mut via = Addr::ZERO;
            let gateway = addr(RTA_GATEWAY);
            via.tag = if gateway.is_ipv4() {
                Addressfamily::INET4
            } else {
                Addressfamily::INET6
            };
            let bytes = octets(gateway);
            via.payload[..bytes.len()].copy_from_slice(&bytes);
            Some(Route {
                cidr: AddrCidr::new(addr(RTA_DST), p[1]),
                via_router: via,
                preferred_until: OptionTimestamp::ZERO,
                expires_at: OptionTimestamp::ZERO,
            })
        })
        .collect()
}

pub(crate) async fn port_addr_add<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addr: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (ip, prefix) = cidr(&frame.read(addr.narrow()?).await?)?;
        let Some(index) = link(&dump(frame, RTM_GETLINK, IFINFOMSG).await?) else {
            return shim::fail(Errno::NODEV);
        };
        // net/ipv4/devinet.c: inet_rtm_newaddr; net/ipv6/addrconf.c: inet6_rtm_newaddr.
        let mut payload = vec![family(ip), prefix, 0, 0];
        payload.extend_from_slice(&index.to_le_bytes());
        rta(&mut payload, IFA_ADDRESS, &octets(ip));
        if ip.is_ipv4() {
            rta(&mut payload, IFA_LOCAL, &octets(ip));
        }
        change(frame, RTM_NEWADDR, NLM_F_CREATE | NLM_F_REPLACE, &payload).await
    })
    .await
}

async fn remove_addresses<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    target: Option<IpAddr>,
) -> V1Result<()> {
    let mut list = dump(frame, RTM_GETADDR, IFADDRMSG).await?;
    // devinet.c: __inet_del_ifa deletes a primary's secondaries as well.
    // Delete those first so clearing the dumped list does not hit stale entries.
    list.sort_by_key(|(_, p)| p.get(2).is_none_or(|flags| flags & 1 == 0));
    for (kind, payload) in list {
        if kind == RTM_NEWADDR
            && address_ip(&payload).is_some_and(|ip| target.is_none_or(|t| ip == t))
        {
            // inet_rtm_deladdr / inet6_rtm_deladdr retain interface, prefix and local/peer.
            change(frame, RTM_DELADDR, 0, &payload).await?;
        }
    }
    Ok(())
}

pub(crate) async fn port_addr_remove<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addr: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let addr: Addr = frame.read(addr.narrow()?).await?;
        remove_addresses(frame, Some(v1_ip(addr.tag, &addr.payload)?)).await
    })
    .await
}

pub(crate) async fn port_addr_clear<S: Syscalls + Control + 'static>(
    env: Env<S>,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| remove_addresses(frame, None).await).await
}

fn route_payload(dst: IpAddr, prefix: u8, gateway: IpAddr) -> V1Result<Vec<u8>> {
    if dst.is_ipv4() != gateway.is_ipv4() {
        return shim::fail(Errno::INVAL);
    }
    // fib_frontend.c: inet_rtm_newroute; ipv6/route.c: inet6_rtm_newroute.
    // Main table, static protocol, universe scope, unicast.
    let mut payload = vec![family(dst), prefix, 0, 0, 254, 4, 0, 1, 0, 0, 0, 0];
    rta(&mut payload, RTA_DST, &octets(dst));
    if !gateway.is_unspecified() {
        rta(&mut payload, RTA_GATEWAY, &octets(gateway));
    }
    Ok(payload)
}

async fn add_route<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    dst: IpAddr,
    prefix: u8,
    gateway: IpAddr,
) -> V1Result<()> {
    let mut payload = route_payload(dst, prefix, gateway)?;
    // On-link and IPv6 link-local routes need an explicit output interface.
    if gateway.is_unspecified() || matches!(gateway, IpAddr::V6(a) if a.is_unicast_link_local()) {
        let Some(index) = link(&dump(frame, RTM_GETLINK, IFINFOMSG).await?) else {
            return shim::fail(Errno::NODEV);
        };
        rta(&mut payload, RTA_OIF, &index.to_le_bytes());
    }
    change(frame, RTM_NEWROUTE, NLM_F_CREATE | NLM_F_REPLACE, &payload).await
}

fn lifetime(value: OptionTimestamp) -> V1Result<()> {
    match value.tag {
        OptionTag::NONE => Ok(()),
        // Kernel routes have no expiry storage. Never acknowledge a discarded deadline.
        OptionTag::SOME => shim::fail(Errno::NOTSUP),
        _ => shim::fail(Errno::INVAL),
    }
}

pub(crate) async fn port_route_add<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addr: Pointer,
    via: Pointer,
    preferred: Pointer,
    expires: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (dst, prefix) = cidr(&frame.read(addr.narrow()?).await?)?;
        let via: Addr = frame.read(via.narrow()?).await?;
        let gateway = v1_ip(via.tag, &via.payload)?;
        lifetime(frame.read(preferred.narrow()?).await?)?;
        lifetime(frame.read(expires.narrow()?).await?)?;
        add_route(frame, dst, prefix, gateway).await
    })
    .await
}

pub(crate) async fn port_gateway_set<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addr: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let addr: Addr = frame.read(addr.narrow()?).await?;
        let gateway = v1_ip(addr.tag, &addr.payload)?;
        let dst = ip(family(gateway), &[0; 16]).unwrap();
        add_route(frame, dst, 0, gateway).await
    })
    .await
}

async fn remove_routes<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    target: Option<IpAddr>,
) -> V1Result<()> {
    for (kind, payload) in dump(frame, RTM_GETROUTE, RTMSG).await? {
        if kind != RTM_NEWROUTE || payload.len() < RTMSG {
            continue;
        }
        let Some(route) = routes(&[(kind, payload.clone())]).pop() else {
            continue;
        };
        let (dst, _) = cidr(&route.cidr)?;
        if target.is_none_or(|t| t == dst) {
            change(frame, RTM_DELROUTE, 0, &payload).await?;
        }
    }
    Ok(())
}

pub(crate) async fn port_route_remove<S: Syscalls + Control + 'static>(
    env: Env<S>,
    addr: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let addr: Addr = frame.read(addr.narrow()?).await?;
        remove_routes(frame, Some(v1_ip(addr.tag, &addr.payload)?)).await
    })
    .await
}

pub(crate) async fn port_route_clear<S: Syscalls + Control + 'static>(
    env: Env<S>,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| remove_routes(frame, None).await).await
}

pub(crate) async fn port_route_list<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: Pointer,
    count: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let room = frame.read_size(count).await?;
        let bytes = room
            .checked_mul(Route::SIZE as u64)
            .ok_or(Errno::OVERFLOW)?;
        frame.check_range(out, bytes).await?;
        let list = routes(&dump(frame, RTM_GETROUTE, RTMSG).await?);
        frame.write_size(count, list.len() as u64).await?;
        if list.len() as u64 > room {
            return shim::fail(Errno::OVERFLOW);
        }
        frame.check_range(out, bytes).await?;
        for (i, route) in list.iter().enumerate() {
            frame
                .write(out.offset((i * Route::SIZE) as u64)?.narrow()?, route)
                .await?;
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nlmsg(kind: u16, payload: &[u8]) -> Vec<u8> {
        let len = NLMSG_HDR + payload.len();
        let mut out = Vec::new();
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.extend_from_slice(payload);
        out.resize(len.next_multiple_of(4), 0);
        out
    }

    fn rta(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((4 + data.len()) as u16).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    #[test]
    fn parses_ipv4_ipv6_and_default_routes() {
        let v4 = route_payload(
            "192.0.2.0".parse().unwrap(),
            24,
            "127.0.0.1".parse().unwrap(),
        )
        .unwrap();
        let v6 = route_payload(
            "2001:db8::".parse().unwrap(),
            64,
            "fe80::1".parse().unwrap(),
        )
        .unwrap();
        let mut default = vec![
            AddressFamily::AF_INET.0 as u8,
            0,
            0,
            0,
            254,
            4,
            0,
            1,
            0,
            0,
            0,
            0,
        ];
        default.extend(rta(RTA_GATEWAY, &[127, 0, 0, 1]));
        let list = routes(&[
            (RTM_NEWROUTE, v4),
            (RTM_NEWROUTE, v6),
            (RTM_NEWROUTE, default),
        ]);
        assert_eq!(list.len(), 3);
        assert_eq!(
            cidr(&list[0].cidr).unwrap(),
            ("192.0.2.0".parse().unwrap(), 24)
        );
        assert_eq!(
            cidr(&list[1].cidr).unwrap(),
            ("2001:db8::".parse().unwrap(), 64)
        );
        assert_eq!(
            v1_ip(list[1].via_router.tag, &list[1].via_router.payload).unwrap(),
            "fe80::1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            cidr(&list[2].cidr).unwrap(),
            ("0.0.0.0".parse().unwrap(), 0)
        );
        assert_eq!(list[0].expires_at, OptionTimestamp::ZERO);
        assert!(route_payload("192.0.2.0".parse().unwrap(), 24, "::1".parse().unwrap()).is_err());
    }

    #[test]
    fn mutations_finish_on_successful_ack() {
        assert_eq!(
            messages(&nlmsg(NLMSG_ERROR, &0i32.to_le_bytes()), &mut Vec::new()),
            Ok(true)
        );
    }

    #[test]
    fn address_removal_uses_local_not_peer() {
        let mut payload = vec![AddressFamily::AF_INET.0 as u8, 24, 0, 0, 2, 0, 0, 0];
        payload.extend(rta(IFA_ADDRESS, &[192, 0, 2, 2]));
        payload.extend(rta(IFA_LOCAL, &[192, 0, 2, 1]));
        assert_eq!(address_ip(&payload), Some("192.0.2.1".parse().unwrap()));
    }

    #[test]
    fn parses_an_address_dump() {
        let mut v4 = vec![AddressFamily::AF_INET.0 as u8, 8, 0, 254, 1, 0, 0, 0];
        v4.extend(rta(IFA_ADDRESS, &[127, 0, 0, 1]));
        let mut v6 = vec![AddressFamily::AF_INET6.0 as u8, 64, 0, 0, 2, 0, 0, 0];
        v6.extend(rta(
            IFA_ADDRESS,
            &[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        ));
        let mut data = nlmsg(RTM_NEWADDR, &v4);
        data.extend(nlmsg(RTM_NEWADDR, &v6));
        data.extend(nlmsg(NLMSG_DONE, &0i32.to_le_bytes()));
        let mut msgs = Vec::new();
        assert_eq!(messages(&data, &mut msgs), Ok(true));
        let list = addresses(&msgs);
        assert_eq!(
            list,
            [
                AddrCidr::new("127.0.0.1".parse().unwrap(), 8),
                AddrCidr::new("fe80::1".parse().unwrap(), 64)
            ]
        );
    }

    #[test]
    fn prefers_a_real_mac() {
        let mut lo = vec![0, 0];
        lo.extend(ARPHRD_LOOPBACK.to_le_bytes());
        lo.extend(1u32.to_le_bytes());
        lo.extend([0; 8]);
        lo.extend(rta(IFLA_ADDRESS, &[0; 6]));
        let mut eth = vec![0, 0, 1, 0];
        eth.extend(2u32.to_le_bytes());
        eth.extend([0; 8]);
        eth.extend(rta(IFLA_ADDRESS, &[2, 0, 0, 0, 0, 2]));
        let msgs = vec![(RTM_NEWLINK, lo), (RTM_NEWLINK, eth)];
        assert_eq!(mac(&msgs), Some([2, 0, 0, 0, 0, 2]));
        assert_eq!(mac(&msgs[..1]), Some([0; 6]));
        assert_eq!(link(&msgs), Some(2));
        assert_eq!(link(&msgs[..1]), Some(1));
    }

    #[test]
    fn errors_end_a_dump() {
        let data = nlmsg(NLMSG_ERROR, &(-95i32).to_le_bytes());
        assert_eq!(
            messages(&data, &mut Vec::new()),
            Err(wasixv2_abi::Errno::EOPNOTSUPP)
        );
    }
}
