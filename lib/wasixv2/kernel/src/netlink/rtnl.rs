//! The `NETLINK_ROUTE` requests (`net/core/rtnetlink.c`, `net/ipv4/devinet.c`,
//! `net/ipv4/fib_frontend.c`, `net/ipv6/addrconf.c`, `net/ipv6/route.c`): links, addresses and
//! routes of the socket's network namespace, read and changed through its [`Network`]. Every
//! function here answers one request; replies come back as encoded messages for the socket to
//! queue, errors as the errno the acknowledgement carries.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use wasixv2_abi::AddressFamily;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::Cap;
use wasixv2_vnet::{
    Interface, Network, NewLink, NewLinkKind,
    iface::{
        AddMode, AddrMatch, IfAddr, Route, RouteDecision, RouteMatch,
        flags::{IFF_LOOPBACK, IFF_LOWER_UP, IFF_UP},
        ifa::IFA_F_PERMANENT,
        rt::{
            RT_SCOPE_HOST, RT_SCOPE_UNIVERSE, RT_TABLE_LOCAL, RTN_LOCAL, RTN_UNICAST, RTN_UNSPEC,
            RTPROT_KERNEL,
        },
    },
};

use super::{NLM_F_MULTI, attrs, nlmsg, rta};
use crate::ns::NetNs;

pub(super) const RTM_NEWLINK: u16 = 16;
pub(super) const RTM_DELLINK: u16 = 17;
pub(super) const RTM_GETLINK: u16 = 18;
pub(super) const RTM_SETLINK: u16 = 19;
pub(super) const RTM_NEWADDR: u16 = 20;
pub(super) const RTM_DELADDR: u16 = 21;
pub(super) const RTM_GETADDR: u16 = 22;
pub(super) const RTM_NEWROUTE: u16 = 24;
pub(super) const RTM_DELROUTE: u16 = 25;
pub(super) const RTM_GETROUTE: u16 = 26;
pub(super) const RTM_GETNEIGH: u16 = 30;
pub(super) const RTM_GETRULE: u16 = 34;
pub(super) const RTM_NEWNSID: u16 = 88;
pub(super) const RTM_GETNSID: u16 = 90;

pub(super) const NLM_F_REPLACE: u16 = 0x100;
pub(super) const NLM_F_EXCL: u16 = 0x200;
pub(super) const NLM_F_CREATE: u16 = 0x400;
pub(super) const NLM_F_APPEND: u16 = 0x800;

pub(super) const IFLA_ADDRESS: u16 = 1;
pub(super) const IFLA_BROADCAST: u16 = 2;
pub(super) const IFLA_IFNAME: u16 = 3;
pub(super) const IFLA_MTU: u16 = 4;
pub(super) const IFLA_LINK: u16 = 5;
const IFLA_QDISC: u16 = 6;
const IFLA_STATS: u16 = 7;
const IFLA_TXQLEN: u16 = 13;
pub(super) const IFLA_OPERSTATE: u16 = 16;
const IFLA_LINKMODE: u16 = 17;
pub(super) const IFLA_LINKINFO: u16 = 18;
pub(super) const IFLA_NET_NS_PID: u16 = 19;
const IFLA_STATS64: u16 = 23;
const IFLA_GROUP: u16 = 27;
const IFLA_NET_NS_FD: u16 = 28;
const IFLA_EXT_MASK: u16 = 29;
const IFLA_PROMISCUITY: u16 = 30;
const IFLA_NUM_TX_QUEUES: u16 = 31;
const IFLA_NUM_RX_QUEUES: u16 = 32;
const IFLA_CARRIER: u16 = 33;
const IFLA_CARRIER_CHANGES: u16 = 35;
pub(super) const IFLA_LINK_NETNSID: u16 = 37;
const IFLA_MIN_MTU: u16 = 50;
const IFLA_MAX_MTU: u16 = 51;
pub(super) const IFLA_INFO_KIND: u16 = 1;
pub(super) const IFLA_INFO_DATA: u16 = 2;
pub(super) const VETH_INFO_PEER: u16 = 1;

pub(super) const IFA_ADDRESS: u16 = 1;
pub(super) const IFA_LOCAL: u16 = 2;
const IFA_LABEL: u16 = 3;
const IFA_BROADCAST: u16 = 4;
const IFA_CACHEINFO: u16 = 6;
const IFA_FLAGS: u16 = 8;
const IFA_RT_PRIORITY: u16 = 9;

const NETNSA_NSID: u16 = 1;
const NETNSA_PID: u16 = 2;
const NETNSA_FD: u16 = 3;
const NETNSA_TARGET_NSID: u16 = 4;
/// `NETNSA_NSID_NOT_ASSIGNED`.
const NSID_NOT_ASSIGNED: i32 = -1;
/// `NLMSG_ALIGN(sizeof(struct rtgenmsg))`.
const RTGENMSG: usize = 4;

pub(super) const RTA_DST: u16 = 1;
pub(super) const RTA_OIF: u16 = 4;
pub(super) const RTA_GATEWAY: u16 = 5;
const RTA_PRIORITY: u16 = 6;
const RTA_PREFSRC: u16 = 7;
const RTA_METRICS: u16 = 8;
const RTA_MULTIPATH: u16 = 9;
const RTA_TABLE: u16 = 15;
const RTA_VIA: u16 = 18;
const RTA_PREF: u16 = 20;
const RTA_NH_ID: u16 = 30;
/// `RTM_F_CLONED`: the answer of a lookup, not a route of a table.
const RTM_F_CLONED: u32 = 0x200;
/// `RT_TABLE_COMPAT`: what `rtm_table` says of a table past 255.
const RT_TABLE_COMPAT: u8 = 252;

const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
const IF_OPER_UNKNOWN: u8 = 0;
const IF_OPER_DOWN: u8 = 2;
const IF_OPER_LOWERLAYERDOWN: u8 = 3;
const IF_OPER_UP: u8 = 6;
/// `NLA_TYPE_MASK`: the flag bits of an attribute's type.
const NLA_TYPE_MASK: u16 = 0x3fff;
/// `INFINITY_LIFE_TIME`.
const FOREVER: u32 = u32::MAX;
/// `DEFAULT_TX_QUEUE_LEN`.
const TXQLEN: u32 = 1000;
/// `sizeof` of `struct ifinfomsg`, `struct ifaddrmsg`, `struct rtmsg`.
pub(super) const IFINFOMSG: usize = 16;
const IFADDRMSG: usize = 8;
const RTMSG: usize = 12;
/// `struct rtnl_link_stats` and `struct rtnl_link_stats64`, all zero: nothing is counted.
const STATS: usize = 24 * 4;
const STATS64: usize = 25 * 8;

/// What a request needs of its sender besides the socket (`netlink_skb_parms`, `current`).
pub(crate) trait Sender {
    /// `get_net_ns_by_pid`: the network namespace of process `pid` (`ESRCH`).
    fn net_of_pid(&self, pid: u32) -> Result<Arc<NetNs>, Errno>;
    /// `get_net_ns_by_fd`: the network namespace an nsfs descriptor stands for (`EBADF`,
    /// `EINVAL` for another file).
    fn net_of_fd(&self, fd: i32) -> Result<Arc<NetNs>, Errno>;
    /// `netlink_ns_capable`: whether the sender and the socket's opener both have `cap` over
    /// the owner of `net`.
    fn net_capable(&self, net: &NetNs, cap: Cap) -> bool;
}

/// A sender that cannot name other namespaces, such as a `write`, which has no caller here.
pub(crate) struct NoNamespaces;

impl Sender for NoNamespaces {
    fn net_of_pid(&self, _: u32) -> Result<Arc<NetNs>, Errno> {
        Err(Errno::EINVAL)
    }

    fn net_of_fd(&self, _: i32) -> Result<Arc<NetNs>, Errno> {
        Err(Errno::EINVAL)
    }

    fn net_capable(&self, _: &NetNs, _: Cap) -> bool {
        false
    }
}

/// One request: the socket's namespace and the header's fields replies echo.
pub(super) struct Request<'a> {
    pub(super) net: &'a NetNs,
    pub(super) seq: u32,
    pub(super) port: u32,
    pub(super) flags: u16,
    pub(super) sender: &'a dyn Sender,
}

impl Request<'_> {
    fn network(&self) -> Result<&Arc<dyn Network>, Errno> {
        self.net.network.as_ref().ok_or(Errno::ENODEV)
    }

    fn interfaces(&self) -> Vec<Interface> {
        interfaces_of(self.net)
    }

    /// A dump: the messages `fill` makes, then `NLMSG_DONE`.
    fn dump(&self, msgs: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = msgs.concat();
        out.extend(nlmsg(
            super::NLMSG_DONE,
            NLM_F_MULTI,
            self.seq,
            self.port,
            &0i32.to_le_bytes(),
        ));
        out
    }

    /// `NLM_F_*` of a change as the tables take them.
    fn mode(&self) -> AddMode {
        AddMode {
            create: self.flags & NLM_F_CREATE != 0,
            excl: self.flags & NLM_F_EXCL != 0,
            replace: self.flags & NLM_F_REPLACE != 0,
            append: self.flags & NLM_F_APPEND != 0,
        }
    }
}

pub(super) fn interfaces_of(net: &NetNs) -> Vec<Interface> {
    net.network
        .as_ref()
        .map(|network| network.interfaces())
        .unwrap_or_default()
}

/// A request's attributes, by type with the flag bits masked off (`nla_parse`); the last of a
/// type wins.
struct Attrs<'a>(Vec<(u16, &'a [u8])>);

impl<'a> Attrs<'a> {
    fn parse(raw: &'a [u8]) -> Self {
        Self(attrs(raw).map(|(k, v)| (k & NLA_TYPE_MASK, v)).collect())
    }

    fn get(&self, kind: u16) -> Option<&'a [u8]> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| *k == kind)
            .map(|&(_, v)| v)
    }

    fn has(&self, kind: u16) -> bool {
        self.get(kind).is_some()
    }

    fn u32(&self, kind: u16) -> Result<Option<u32>, Errno> {
        self.get(kind)
            .map(|v| {
                v.get(..4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .ok_or(Errno::EINVAL)
            })
            .transpose()
    }

    /// A NUL-terminated string (`nla_strscpy`), at most `IFNAMSIZ - 1` bytes for a name.
    fn str(&self, kind: u16) -> Result<Option<String>, Errno> {
        self.get(kind)
            .map(|v| {
                let bytes = v.split(|&b| b == 0).next().unwrap_or(v);
                String::from_utf8(bytes.to_vec()).map_err(|_| Errno::EINVAL)
            })
            .transpose()
    }

    fn mac(&self, kind: u16) -> Result<Option<[u8; 6]>, Errno> {
        self.get(kind)
            .map(|v| {
                v.get(..6)
                    .map(|b| b.try_into().unwrap())
                    .ok_or(Errno::EINVAL)
            })
            .transpose()
    }

    /// An address of `family` (`nla_get_in_addr`, `nla_get_in6_addr`).
    fn ip(&self, kind: u16, v6: bool) -> Result<Option<IpAddr>, Errno> {
        self.get(kind).map(|v| ip_of(v, v6)).transpose()
    }
}

fn ip_of(raw: &[u8], v6: bool) -> Result<IpAddr, Errno> {
    match v6 {
        false => raw
            .get(..4)
            .map(|b| IpAddr::from(<[u8; 4]>::try_from(b).unwrap()))
            .ok_or(Errno::EINVAL),
        true => raw
            .get(..16)
            .map(|b| IpAddr::from(<[u8; 16]>::try_from(b).unwrap()))
            .ok_or(Errno::EINVAL),
    }
}

fn octets(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

fn family_of(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => AddressFamily::AF_INET.0 as u8,
        IpAddr::V6(_) => AddressFamily::AF_INET6.0 as u8,
    }
}

/// `AF_INET` or `AF_INET6` as "is it IPv6"; another family has no handler (`EOPNOTSUPP`, as
/// `rtnetlink_rcv_msg` finds none for it).
fn inet_family(family: u8) -> Result<bool, Errno> {
    match family as i32 {
        f if f == AddressFamily::AF_INET.0 => Ok(false),
        f if f == AddressFamily::AF_INET6.0 => Ok(true),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `rtnl_getlink`'s lookup: the interface a request names by index or by `IFLA_IFNAME`.
pub(super) fn find_link<'a>(
    interfaces: &'a [Interface],
    payload: &[u8],
) -> Result<&'a Interface, Errno> {
    let ifinfo = payload.get(..IFINFOMSG).ok_or(Errno::EINVAL)?;
    let index = i32::from_le_bytes(ifinfo[4..8].try_into().unwrap());
    let found = if index > 0 {
        interfaces.iter().find(|i| i.index as i32 == index)
    } else {
        let name = Attrs::parse(&payload[IFINFOMSG..])
            .str(IFLA_IFNAME)?
            .ok_or(Errno::EINVAL)?;
        interfaces.iter().find(|i| i.name == name)
    };
    found.ok_or(Errno::ENODEV)
}

/// `RTM_GETLINK`: one interface, or with `dump` every one.
pub(super) fn get_link(r: &Request<'_>, payload: &[u8], dump: bool) -> Result<Vec<u8>, Errno> {
    let interfaces = r.interfaces();
    if dump {
        let msgs = interfaces
            .iter()
            .map(|i| link_msg(i, r.seq, r.port, NLM_F_MULTI))
            .collect();
        return Ok(r.dump(msgs));
    }
    let iface = find_link(&interfaces, payload)?;
    Ok(link_msg(iface, r.seq, r.port, 0))
}

/// `RTM_NEWLINK` (and `RTM_SETLINK`, which only changes): `rtnl_newlink`. An interface that
/// exists changes as `do_setlink` changes it, in its order: namespace, MAC address, MTU, name,
/// flags. One that does not is made with `NLM_F_CREATE` (`rtnl_newlink_create`): a `dummy` or
/// a `veth` pair, configured the same way after.
pub(super) fn new_link(r: &Request<'_>, payload: &[u8], set_only: bool) -> Result<(), Errno> {
    let ifinfo = payload.get(..IFINFOMSG).ok_or(Errno::EINVAL)?;
    let index = i32::from_le_bytes(ifinfo[4..8].try_into().unwrap());
    let flags = u32::from_le_bytes(ifinfo[8..12].try_into().unwrap());
    let change = u32::from_le_bytes(ifinfo[12..16].try_into().unwrap());
    if index < 0 {
        return Err(Errno::EINVAL);
    }
    let attrs = Attrs::parse(&payload[IFINFOMSG..]);
    check_link_attrs(&attrs)?;
    let interfaces = r.interfaces();
    let named = attrs.has(IFLA_IFNAME);
    let existing = match index > 0 || named {
        true => find_link(&interfaces, payload).ok().cloned(),
        false => None,
    };
    let kind = link_kind(&attrs)?;
    let iface = match existing {
        Some(iface) => {
            if set_only {
                // `rtnl_setlink` has no create or exclusive flags to honour.
            } else if r.flags & NLM_F_EXCL != 0 {
                return Err(Errno::EEXIST);
            } else if r.flags & NLM_F_REPLACE != 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            if kind
                .as_ref()
                .is_some_and(|(k, data)| data.is_some() && Some(*k) != iface.kind)
            {
                return Err(Errno::EOPNOTSUPP);
            }
            iface
        }
        None if set_only || r.flags & NLM_F_CREATE == 0 => return Err(Errno::ENODEV),
        None => {
            let (kind, data) = kind.ok_or(Errno::EOPNOTSUPP)?;
            return create_link(r, &attrs, kind, data, flags, change);
        }
    };
    set_link(r, &iface, index > 0, &attrs, flags, change)
}

/// The attributes a link request may carry; others ask for what is not here (`EOPNOTSUPP`).
fn check_link_attrs(attrs: &Attrs<'_>) -> Result<(), Errno> {
    let known = [
        IFLA_ADDRESS,
        IFLA_IFNAME,
        IFLA_MTU,
        IFLA_LINKINFO,
        IFLA_NET_NS_PID,
        IFLA_NET_NS_FD,
        IFLA_TXQLEN,
        IFLA_EXT_MASK,
    ];
    for &(kind, value) in &attrs.0 {
        // A group of 0 is the default group every interface is in.
        let default_group = kind == IFLA_GROUP && value.get(..4) == Some(&[0; 4]);
        if !known.contains(&kind) && !default_group {
            return Err(Errno::EOPNOTSUPP);
        }
    }
    Ok(())
}

/// A link's kind (`IFLA_INFO_KIND`) and its kind's attributes (`IFLA_INFO_DATA`).
type LinkKind<'a> = (&'static str, Option<&'a [u8]>);

/// `IFLA_LINKINFO`'s `IFLA_INFO_KIND` and `IFLA_INFO_DATA`, if given; an unknown kind is
/// `EOPNOTSUPP` (no module to load).
fn link_kind<'a>(attrs: &Attrs<'a>) -> Result<Option<LinkKind<'a>>, Errno> {
    let Some(info) = attrs.get(IFLA_LINKINFO) else {
        return Ok(None);
    };
    let info = Attrs::parse(info);
    let Some(kind) = info.str(IFLA_INFO_KIND)? else {
        return Ok(None);
    };
    let kind = match kind.as_str() {
        "veth" => "veth",
        "dummy" => "dummy",
        _ => return Err(Errno::EOPNOTSUPP),
    };
    Ok(Some((kind, info.get(IFLA_INFO_DATA))))
}

/// The namespace `IFLA_NET_NS_PID` or `IFLA_NET_NS_FD` names, if either does, which the
/// sender needs `CAP_NET_ADMIN` over (`rtnl_link_get_net_capable`, and `rtnl_get_peer_net`
/// for a veth's peer).
fn target_net(r: &Request<'_>, attrs: &Attrs<'_>) -> Result<Option<Arc<NetNs>>, Errno> {
    let net = if let Some(pid) = attrs.u32(IFLA_NET_NS_PID)? {
        r.sender.net_of_pid(pid)?
    } else if let Some(fd) = attrs.u32(IFLA_NET_NS_FD)? {
        r.sender.net_of_fd(fd as i32)?
    } else {
        return Ok(None);
    };
    if !r.sender.net_capable(&net, Cap::NetAdmin) {
        return Err(Errno::EPERM);
    }
    Ok(Some(net))
}

/// `rtnl_newlink_create` and `veth_newlink`: makes the interface (a veth's peer first, from
/// `VETH_INFO_PEER`'s own `ifinfomsg` and attributes), moves each to the namespace its request
/// names, then applies the flags (`rtnl_configure_link`).
fn create_link(
    r: &Request<'_>,
    attrs: &Attrs<'_>,
    kind: &str,
    data: Option<&[u8]>,
    flags: u32,
    change: u32,
) -> Result<(), Errno> {
    let network = r.network()?;
    let mut peer_net = None;
    let new_kind = match kind {
        "veth" => {
            let peer = data
                .map(Attrs::parse)
                .and_then(|d| d.get(VETH_INFO_PEER).map(<[u8]>::to_vec));
            let (peer_name, peer_mtu, peer_mac) = match &peer {
                Some(peer) => {
                    let peer_attrs = Attrs::parse(peer.get(IFINFOMSG..).ok_or(Errno::EINVAL)?);
                    check_link_attrs(&peer_attrs)?;
                    peer_net = target_net(r, &peer_attrs)?;
                    (
                        peer_attrs.str(IFLA_IFNAME)?,
                        peer_attrs.u32(IFLA_MTU)?,
                        peer_attrs.mac(IFLA_ADDRESS)?,
                    )
                }
                None => (None, None, None),
            };
            NewLinkKind::Veth {
                peer_name,
                peer_mtu,
                peer_mac,
            }
        }
        _ => {
            if data.is_some_and(|d| !d.is_empty()) {
                return Err(Errno::EINVAL);
            }
            NewLinkKind::Dummy
        }
    };
    let dest = target_net(r, attrs)?;
    let (index, peer) = network.new_link(NewLink {
        name: attrs.str(IFLA_IFNAME)?,
        mtu: attrs.u32(IFLA_MTU)?,
        mac: attrs.mac(IFLA_ADDRESS)?,
        kind: new_kind,
    })?;
    let undo = || {
        let _ = network.del_link(index);
    };
    let moved = |index: u32, to: &Option<Arc<NetNs>>| -> Result<(Arc<dyn Network>, u32), Errno> {
        match to {
            Some(to) if !std::ptr::eq(Arc::as_ptr(to), r.net) => {
                let there = to.network.clone().ok_or(Errno::EINVAL)?;
                let index = wasixv2_vnet::move_link(&**network, index, &*there)?;
                Ok((there, index))
            }
            _ => Ok((network.clone(), index)),
        }
    };
    if let Some(peer) = peer
        && let Err(e) = moved(peer, &peer_net)
    {
        undo();
        return Err(e);
    }
    let (home, index) = match moved(index, &dest) {
        Ok(at) => at,
        Err(e) => {
            undo();
            return Err(e);
        }
    };
    if flags != 0 || change != 0 {
        let current = home
            .interfaces()
            .into_iter()
            .find(|i| i.index == index)
            .map_or(0, |i| i.flags);
        home.change_flags(index, combine_flags(current, flags, change))?;
    }
    Ok(())
}

/// `rtnl_dev_combine_flags`: `ifi_change` names the flags `ifi_flags` sets; 0 means all.
fn combine_flags(current: u32, flags: u32, change: u32) -> u32 {
    match change {
        0 => flags,
        change => (flags & change) | (current & !change),
    }
}

/// `do_setlink` on interface `iface`; `by_index` is whether the request named it by index,
/// so that `IFLA_IFNAME` renames it.
fn set_link(
    r: &Request<'_>,
    iface: &Interface,
    by_index: bool,
    attrs: &Attrs<'_>,
    flags: u32,
    change: u32,
) -> Result<(), Errno> {
    let mut network = r.network()?.clone();
    let mut index = iface.index;
    if let Some(to) = target_net(r, attrs)?
        && !std::ptr::eq(Arc::as_ptr(&to), r.net)
    {
        let there = to.network.clone().ok_or(Errno::EINVAL)?;
        index = wasixv2_vnet::move_link(&*network, index, &*there)?;
        network = there;
    }
    if let Some(mac) = attrs.mac(IFLA_ADDRESS)? {
        network.set_mac(index, mac)?;
    }
    if let Some(mtu) = attrs.u32(IFLA_MTU)? {
        network.set_mtu(index, mtu)?;
    }
    if by_index && let Some(name) = attrs.str(IFLA_IFNAME)? {
        network.rename_link(index, &name)?;
    }
    if flags != 0 || change != 0 {
        let current = network
            .interfaces()
            .into_iter()
            .find(|i| i.index == index)
            .map_or(iface.flags, |i| i.flags);
        network.change_flags(index, combine_flags(current, flags, change))?;
    }
    Ok(())
}

/// `RTM_DELLINK` (`rtnl_dellink`): by index or name; `EINVAL` naming none, `EOPNOTSUPP` for an
/// interface no request made.
pub(super) fn del_link(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let ifinfo = payload.get(..IFINFOMSG).ok_or(Errno::EINVAL)?;
    let index = i32::from_le_bytes(ifinfo[4..8].try_into().unwrap());
    let attrs = Attrs::parse(&payload[IFINFOMSG..]);
    if index <= 0 && !attrs.has(IFLA_IFNAME) {
        return Err(Errno::EINVAL);
    }
    let interfaces = r.interfaces();
    let iface = find_link(&interfaces, payload)?;
    r.network()?.del_link(iface.index)
}

/// `IF_OPER_*`, as `rfc2863_policy` derives it: down, up, up without carrier; loopback and
/// devices that never report a carrier (dummy) stay unknown.
fn operstate(iface: &Interface) -> u8 {
    if iface.flags & IFF_UP == 0 {
        return IF_OPER_DOWN;
    }
    match (iface.flags & IFF_LOOPBACK != 0, iface.kind) {
        (true, _) | (_, Some("dummy")) => IF_OPER_UNKNOWN,
        _ if iface.flags & IFF_LOWER_UP != 0 => IF_OPER_UP,
        _ => IF_OPER_LOWERLAYERDOWN,
    }
}

/// `rtnl_fill_ifinfo`: `struct ifinfomsg` and the attributes `ip link` shows. Nothing is
/// counted, so the statistics are zero.
pub(super) fn link_msg(iface: &Interface, seq: u32, port: u32, flags: u16) -> Vec<u8> {
    let loopback = iface.flags & IFF_LOOPBACK != 0;
    let mut p = vec![0, 0]; // ifi_family, padding
    let kind = match loopback {
        true => ARPHRD_LOOPBACK,
        false => ARPHRD_ETHER,
    };
    p.extend(kind.to_le_bytes());
    p.extend((iface.index as i32).to_le_bytes());
    p.extend(iface.flags.to_le_bytes());
    p.extend(0u32.to_le_bytes()); // ifi_change
    let mut name = iface.name.as_bytes().to_vec();
    name.push(0);
    rta(&mut p, IFLA_IFNAME, &name);
    rta(&mut p, IFLA_TXQLEN, &TXQLEN.to_le_bytes());
    rta(&mut p, IFLA_OPERSTATE, &[operstate(iface)]);
    rta(&mut p, IFLA_LINKMODE, &[0]);
    rta(&mut p, IFLA_MTU, &iface.mtu.to_le_bytes());
    rta(&mut p, IFLA_MIN_MTU, &0u32.to_le_bytes());
    rta(&mut p, IFLA_MAX_MTU, &0u32.to_le_bytes());
    rta(&mut p, IFLA_GROUP, &0u32.to_le_bytes());
    rta(&mut p, IFLA_PROMISCUITY, &0u32.to_le_bytes());
    rta(&mut p, IFLA_NUM_TX_QUEUES, &1u32.to_le_bytes());
    rta(&mut p, IFLA_NUM_RX_QUEUES, &1u32.to_le_bytes());
    rta(
        &mut p,
        IFLA_CARRIER,
        &[u8::from(iface.flags & IFF_LOWER_UP != 0)],
    );
    rta(&mut p, IFLA_QDISC, b"noqueue\0");
    rta(&mut p, IFLA_CARRIER_CHANGES, &0u32.to_le_bytes());
    rta(&mut p, IFLA_ADDRESS, &iface.mac);
    let broadcast = match loopback {
        true => [0; 6],
        false => [0xff; 6],
    };
    rta(&mut p, IFLA_BROADCAST, &broadcast);
    rta(&mut p, IFLA_STATS, &[0; STATS]);
    rta(&mut p, IFLA_STATS64, &[0; STATS64]);
    if let Some((peer, nsid)) = iface.peer {
        rta(&mut p, IFLA_LINK, &peer.to_le_bytes());
        if let Some(nsid) = nsid {
            rta(&mut p, IFLA_LINK_NETNSID, &nsid.to_le_bytes());
        }
    }
    if let Some(kind) = iface.kind {
        let mut info = Vec::new();
        let mut kind = kind.as_bytes().to_vec();
        kind.push(0);
        rta(&mut info, IFLA_INFO_KIND, &kind);
        rta(&mut p, IFLA_LINKINFO, &info);
    }
    nlmsg(RTM_NEWLINK, flags, seq, port, &p)
}

/// `RTM_GETADDR`: a dump of the addresses of `family` (all for `AF_UNSPEC`), of one interface
/// if `ifa_index` says so. A dump is all there is (`inet_dump_ifaddr`).
pub(super) fn get_addr(r: &Request<'_>, payload: &[u8], dump: bool) -> Result<Vec<u8>, Errno> {
    if !dump {
        return Err(Errno::EOPNOTSUPP);
    }
    let family = payload.first().copied().unwrap_or(0);
    let index = payload
        .get(4..8)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    let mut msgs = Vec::new();
    for iface in r.interfaces() {
        if index != 0 && iface.index != index {
            continue;
        }
        for a in &iface.addresses {
            let wanted = match family as i32 {
                0 => true,
                f if f == AddressFamily::AF_INET.0 => a.addr.is_ipv4(),
                f if f == AddressFamily::AF_INET6.0 => a.addr.is_ipv6(),
                _ => false,
            };
            if wanted {
                msgs.push(addr_msg(&iface, a, r.seq, r.port));
            }
        }
    }
    Ok(r.dump(msgs))
}

/// `inet_fill_ifaddr` and `inet6_fill_ifaddr`.
fn addr_msg(iface: &Interface, a: &IfAddr, seq: u32, port: u32) -> Vec<u8> {
    let mut p = vec![family_of(a.addr), a.prefix_len, a.flags as u8, a.scope];
    p.extend(iface.index.to_le_bytes());
    match a.addr {
        IpAddr::V4(_) => {
            rta(&mut p, IFA_ADDRESS, &octets(a.peer.unwrap_or(a.addr)));
            rta(&mut p, IFA_LOCAL, &octets(a.addr));
            if let Some(brd) = a.broadcast {
                rta(&mut p, IFA_BROADCAST, &brd.octets());
            }
            let mut label = a
                .label
                .clone()
                .unwrap_or_else(|| iface.name.clone())
                .into_bytes();
            label.push(0);
            rta(&mut p, IFA_LABEL, &label);
        }
        IpAddr::V6(_) => rta(&mut p, IFA_ADDRESS, &octets(a.addr)),
    }
    if a.metric != 0 {
        rta(&mut p, IFA_RT_PRIORITY, &a.metric.to_le_bytes());
    }
    // `struct ifa_cacheinfo`: permanent, made at boot as far as anyone can tell.
    let mut cache = Vec::new();
    for v in [FOREVER, FOREVER, 0, 0] {
        cache.extend(v.to_le_bytes());
    }
    rta(&mut p, IFA_CACHEINFO, &cache);
    rta(&mut p, IFA_FLAGS, &a.flags.to_le_bytes());
    nlmsg(RTM_NEWADDR, NLM_F_MULTI, seq, port, &p)
}

/// `RTM_NEWADDR` (`inet_rtm_newaddr`, `inet6_rtm_newaddr`).
pub(super) fn new_addr(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let msg = payload.get(..IFADDRMSG).ok_or(Errno::EINVAL)?;
    let v6 = inet_family(msg[0])?;
    let (prefix_len, flags, scope) = (msg[1], msg[2], msg[3]);
    let index = u32::from_le_bytes(msg[4..8].try_into().unwrap());
    let attrs = Attrs::parse(&payload[IFADDRMSG..]);
    if prefix_len > if v6 { 128 } else { 32 } {
        return Err(Errno::EINVAL);
    }
    let local = attrs.ip(IFA_LOCAL, v6)?;
    let address = attrs.ip(IFA_ADDRESS, v6)?;
    let (addr, peer) = match (local, address) {
        (Some(l), Some(a)) if l != a => (l, Some(a)),
        (Some(a), _) | (None, Some(a)) => (a, None),
        (None, None) => return Err(Errno::EINVAL),
    };
    let interfaces = r.interfaces();
    let iface = interfaces
        .iter()
        .find(|i| i.index == index)
        .ok_or(Errno::ENODEV)?;
    let flags = attrs.u32(IFA_FLAGS)?.unwrap_or(u32::from(flags));
    let label = attrs.str(IFA_LABEL)?.filter(|l| *l != iface.name);
    let broadcast = match attrs.ip(IFA_BROADCAST, false)? {
        Some(IpAddr::V4(b)) if !v6 => Some(b),
        _ => None,
    };
    let a = IfAddr {
        addr,
        prefix_len,
        peer,
        broadcast,
        scope,
        flags: flags | IFA_F_PERMANENT,
        label,
        metric: attrs.u32(IFA_RT_PRIORITY)?.unwrap_or(0),
    };
    r.network()?.add_address(index, a, r.mode())
}

/// `RTM_DELADDR` (`inet_rtm_deladdr`, `inet6_rtm_deladdr`).
pub(super) fn del_addr(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let msg = payload.get(..IFADDRMSG).ok_or(Errno::EINVAL)?;
    let v6 = inet_family(msg[0])?;
    let index = u32::from_le_bytes(msg[4..8].try_into().unwrap());
    let attrs = Attrs::parse(&payload[IFADDRMSG..]);
    let which = AddrMatch {
        v6,
        local: attrs.ip(IFA_LOCAL, v6)?,
        address: attrs.ip(IFA_ADDRESS, v6)?,
        prefix_len: msg[1],
        label: attrs.str(IFA_LABEL)?,
    };
    if v6 && which.local.is_none() && which.address.is_none() {
        return Err(Errno::EINVAL);
    }
    if !r.interfaces().iter().any(|i| i.index == index) {
        return Err(Errno::ENODEV);
    }
    r.network()?.del_address(index, &which)
}

/// A route request's `struct rtmsg` and attributes (`rtm_to_fib_config`,
/// `rtm_to_fib6_config`).
struct RouteReq {
    v6: bool,
    table: u32,
    dst: IpAddr,
    prefix_len: u8,
    gateway: Option<IpAddr>,
    oif: Option<u32>,
    prefsrc: Option<IpAddr>,
    metric: Option<u32>,
    protocol: u8,
    scope: u8,
    kind: u8,
}

fn route_req(payload: &[u8]) -> Result<RouteReq, Errno> {
    let msg = payload.get(..RTMSG).ok_or(Errno::EINVAL)?;
    let v6 = inet_family(msg[0])?;
    let attrs = Attrs::parse(&payload[RTMSG..]);
    if attrs.has(RTA_MULTIPATH) || attrs.has(RTA_NH_ID) || attrs.has(RTA_VIA) {
        return Err(Errno::EOPNOTSUPP);
    }
    let any = match v6 {
        false => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        true => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    Ok(RouteReq {
        v6,
        table: attrs.u32(RTA_TABLE)?.unwrap_or(u32::from(msg[4])),
        dst: attrs.ip(RTA_DST, v6)?.unwrap_or(any),
        prefix_len: msg[1],
        gateway: attrs.ip(RTA_GATEWAY, v6)?,
        oif: attrs.u32(RTA_OIF)?,
        prefsrc: attrs.ip(RTA_PREFSRC, v6)?,
        metric: attrs.u32(RTA_PRIORITY)?,
        protocol: msg[5],
        scope: msg[6],
        kind: msg[7],
    })
}

/// `RTM_NEWROUTE` (`inet_rtm_newroute`, `inet6_rtm_newroute`). Metrics (`RTA_METRICS`) are
/// accepted and have no effect: nothing here is sent in packets.
pub(super) fn new_route(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let q = route_req(payload)?;
    let _ = RTA_METRICS;
    if let Some(oif) = q.oif
        && !r.interfaces().iter().any(|i| i.index == oif)
    {
        return Err(Errno::ENODEV);
    }
    let route = Route {
        table: q.table,
        dst: q.dst,
        prefix_len: q.prefix_len,
        gateway: q.gateway,
        oif: q.oif.unwrap_or(0),
        prefsrc: q.prefsrc,
        metric: q.metric.unwrap_or(0),
        protocol: q.protocol,
        scope: q.scope,
        kind: match q.kind {
            RTN_UNSPEC => RTN_UNICAST,
            k => k,
        },
    };
    r.network()?.add_route(route, r.mode())
}

/// `RTM_DELROUTE` (`inet_rtm_delroute`, `inet6_rtm_delroute`).
pub(super) fn del_route(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let q = route_req(payload)?;
    let which = RouteMatch {
        table: q.table,
        dst: q.dst,
        prefix_len: q.prefix_len,
        gateway: q.gateway,
        oif: q.oif,
        prefsrc: q.prefsrc,
        metric: q.metric,
        protocol: Some(q.protocol),
        scope: match q.v6 {
            true => None,
            false => Some(q.scope),
        },
        kind: Some(q.kind),
    };
    r.network()?.del_route(&which)
}

/// `RTM_GETROUTE`: a dump of the tables (`inet_dump_fib`, `inet6_dump_fib`), or the lookup a
/// socket would make for `RTA_DST` (`inet_rtm_getroute`), answered as a cloned route.
pub(super) fn get_route(r: &Request<'_>, payload: &[u8], dump: bool) -> Result<Vec<u8>, Errno> {
    let family = payload.first().copied().ok_or(Errno::EINVAL)?;
    if dump {
        let routes = r
            .net
            .network
            .as_ref()
            .map(|n| n.routes())
            .unwrap_or_default();
        let msgs = routes
            .iter()
            .filter(|route| family == 0 || family == family_of(route.dst))
            .map(|route| route_msg(route, r.seq, r.port, NLM_F_MULTI, 0))
            .collect();
        return Ok(r.dump(msgs));
    }
    let q = route_req(payload)?;
    let network = r.net.network.as_ref().ok_or(Errno::ENETUNREACH)?;
    let decision = network.route_get(q.dst)?;
    let max = if q.v6 { 128 } else { 32 };
    let route = lookup_answer(&decision, q.dst, max, &r.interfaces());
    Ok(route_msg(&route, r.seq, r.port, 0, RTM_F_CLONED))
}

/// What `ip route get` shows for a decision: the destination as a host route through the
/// interface the lookup chose, from the source it chose; a local destination is a local route
/// of the local table through loopback.
fn lookup_answer(d: &RouteDecision, dst: IpAddr, max: u8, interfaces: &[Interface]) -> Route {
    let lo = interfaces
        .iter()
        .find(|i| i.flags & IFF_LOOPBACK != 0)
        .map_or(1, |i| i.index);
    match (&d.route, d.local) {
        (Some(route), false) => Route {
            dst,
            prefix_len: max,
            prefsrc: Some(d.src),
            ..route.clone()
        },
        _ => Route {
            table: RT_TABLE_LOCAL,
            dst,
            prefix_len: max,
            gateway: None,
            oif: lo,
            prefsrc: Some(d.src),
            metric: 0,
            protocol: RTPROT_KERNEL,
            scope: RT_SCOPE_HOST,
            kind: RTN_LOCAL,
        },
    }
}

/// `fib_dump_info` and `rt6_fill_node`.
fn route_msg(route: &Route, seq: u32, port: u32, flags: u16, rtm_flags: u32) -> Vec<u8> {
    let v6 = route.dst.is_ipv6();
    let table = u8::try_from(route.table).unwrap_or(RT_TABLE_COMPAT);
    let scope = match v6 {
        true => RT_SCOPE_UNIVERSE,
        false => route.scope,
    };
    let mut p = vec![
        family_of(route.dst),
        route.prefix_len,
        0, // rtm_src_len
        0, // rtm_tos
        table,
        route.protocol,
        scope,
        route.kind,
    ];
    p.extend(rtm_flags.to_le_bytes());
    rta(&mut p, RTA_TABLE, &route.table.to_le_bytes());
    if route.prefix_len > 0 {
        rta(&mut p, RTA_DST, &octets(route.dst));
    }
    if v6 || route.metric != 0 {
        rta(&mut p, RTA_PRIORITY, &route.metric.to_le_bytes());
    }
    if let Some(src) = route.prefsrc {
        rta(&mut p, RTA_PREFSRC, &octets(src));
    }
    if let Some(gw) = route.gateway {
        rta(&mut p, RTA_GATEWAY, &octets(gw));
    }
    if route.oif != 0 {
        rta(&mut p, RTA_OIF, &route.oif.to_le_bytes());
    }
    if v6 {
        rta(&mut p, RTA_PREF, &[0]); // ICMPV6_ROUTER_PREF_MEDIUM
    }
    nlmsg(RTM_NEWROUTE, flags, seq, port, &p)
}

/// The namespace `NETNSA_PID` or `NETNSA_FD` names, if either does.
fn nsid_peer(r: &Request<'_>, attrs: &Attrs<'_>) -> Result<Option<Arc<dyn Network>>, Errno> {
    let peer = if let Some(pid) = attrs.u32(NETNSA_PID)? {
        r.sender.net_of_pid(pid)?
    } else if let Some(fd) = attrs.u32(NETNSA_FD)? {
        r.sender.net_of_fd(fd as i32)?
    } else {
        return Ok(None);
    };
    peer.network.clone().map(Some).ok_or(Errno::EINVAL)
}

/// `RTM_NEWNSID` with the id.
fn nsid_msg(id: i32, seq: u32, port: u32, flags: u16) -> Vec<u8> {
    let mut p = vec![0; RTGENMSG];
    rta(&mut p, NETNSA_NSID, &id.to_le_bytes());
    nlmsg(RTM_NEWNSID, flags, seq, port, &p)
}

/// `RTM_GETNSID` (`rtnl_net_getid`, `rtnl_net_dumpid`): the id this namespace knows the one
/// `NETNSA_PID` or `NETNSA_FD` names by (`NETNSA_NSID_NOT_ASSIGNED` if none), or every id.
/// Asking another namespace (`NETNSA_TARGET_NSID`) is not supported.
pub(super) fn get_nsid(r: &Request<'_>, payload: &[u8], dump: bool) -> Result<Vec<u8>, Errno> {
    let attrs = Attrs::parse(payload.get(RTGENMSG..).unwrap_or_default());
    if attrs.has(NETNSA_TARGET_NSID) {
        return Err(Errno::EOPNOTSUPP);
    }
    let network = r.network()?;
    if dump {
        let msgs = network
            .peer_ids()
            .into_iter()
            .map(|id| nsid_msg(id, r.seq, r.port, NLM_F_MULTI))
            .collect();
        return Ok(r.dump(msgs));
    }
    let id = match nsid_peer(r, &attrs)? {
        Some(peer) => network.peer_id(&*peer).unwrap_or(NSID_NOT_ASSIGNED),
        None => match attrs.u32(NETNSA_NSID)? {
            Some(id) if network.peer_ids().contains(&(id as i32)) => id as i32,
            Some(_) => NSID_NOT_ASSIGNED,
            None => return Err(Errno::EINVAL),
        },
    };
    Ok(nsid_msg(id, r.seq, r.port, 0))
}

/// `RTM_NEWNSID` (`rtnl_net_newid`, `ip netns set`): a negative id takes the lowest free one.
pub(super) fn new_nsid(r: &Request<'_>, payload: &[u8]) -> Result<(), Errno> {
    let attrs = Attrs::parse(payload.get(RTGENMSG..).unwrap_or_default());
    let id = attrs.u32(NETNSA_NSID)?.ok_or(Errno::EINVAL)? as i32;
    let peer = nsid_peer(r, &attrs)?.ok_or(Errno::EINVAL)?;
    let id = (id >= 0).then_some(id);
    r.network()?.assign_peer_id(&*peer, id).map(|_| ())
}

/// `RTM_GETRULE` (`fib_nl_dumprule`): the default rules, which are all there are, and which
/// route lookups follow: local, then main, then (IPv4) default. Rules cannot change.
pub(super) fn get_rule(r: &Request<'_>, payload: &[u8], dump: bool) -> Result<Vec<u8>, Errno> {
    use wasixv2_vnet::iface::rt::{RT_TABLE_DEFAULT, RT_TABLE_MAIN};
    const FRA_PRIORITY: u16 = 6;
    const FRA_SUPPRESS_PREFIXLEN: u16 = 14;
    const FRA_TABLE: u16 = 15;
    const FRA_PROTOCOL: u16 = 21;
    const FR_ACT_TO_TBL: u8 = 1;
    if !dump {
        return Err(Errno::EOPNOTSUPP);
    }
    let family = payload.first().copied().unwrap_or(0);
    let mut msgs = Vec::new();
    for (fam, v6) in [
        (AddressFamily::AF_INET.0 as u8, false),
        (AddressFamily::AF_INET6.0 as u8, true),
    ] {
        if family != 0 && family != fam {
            continue;
        }
        let mut rules = vec![(0u32, RT_TABLE_LOCAL), (32766, RT_TABLE_MAIN)];
        if !v6 {
            rules.push((32767, RT_TABLE_DEFAULT));
        }
        for (pref, table) in rules {
            let mut p = vec![fam, 0, 0, 0, table as u8, 0, 0, FR_ACT_TO_TBL];
            p.extend(0u32.to_le_bytes());
            rta(&mut p, FRA_TABLE, &table.to_le_bytes());
            rta(&mut p, FRA_SUPPRESS_PREFIXLEN, &u32::MAX.to_le_bytes());
            rta(&mut p, FRA_PROTOCOL, &[RTPROT_KERNEL]);
            if pref != 0 {
                rta(&mut p, FRA_PRIORITY, &pref.to_le_bytes());
            }
            msgs.push(nlmsg(RTM_NEWRULE, NLM_F_MULTI, r.seq, r.port, &p));
        }
    }
    Ok(r.dump(msgs))
}

/// `RTM_NEWRULE`, which rule messages are.
const RTM_NEWRULE: u16 = 32;

/// `RTM_GETNEIGH`: no neighbour is ever resolved, so a dump is empty.
pub(super) fn get_neigh(r: &Request<'_>, dump: bool) -> Result<Vec<u8>, Errno> {
    match dump {
        true => Ok(r.dump(Vec::new())),
        false => Err(Errno::ENOENT),
    }
}
