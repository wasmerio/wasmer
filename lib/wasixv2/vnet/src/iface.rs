//! Interfaces, addresses and routes of a namespace. These are plain tables: no frames move
//! through them. They change as Linux's do (`net/core/dev.c`, `net/ipv4/devinet.c`,
//! `net/ipv4/fib_semantics.c`, `net/ipv4/fib_trie.c`, `net/ipv6/addrconf.c`,
//! `net/ipv6/route.c`); the kernel serves them over netlink and `SIOC*`, and the socket layer
//! uses them to decide which addresses are local and where a destination goes.
//!
//! What Linux does with packets is left out: there is no neighbour discovery, and duplicate
//! address detection succeeds at once (no address is ever `IFA_F_TENTATIVE`), since nothing on a
//! link could answer.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex, Weak},
};

use wasixv2_abi::Errno;

use crate::{Net, NetResult, sync::MutexExt};

/// `IFF_*` flags (`include/uapi/linux/if.h`).
pub mod flags {
    pub const IFF_UP: u32 = 0x1;
    pub const IFF_BROADCAST: u32 = 0x2;
    pub const IFF_DEBUG: u32 = 0x4;
    pub const IFF_LOOPBACK: u32 = 0x8;
    pub const IFF_POINTOPOINT: u32 = 0x10;
    pub const IFF_NOTRAILERS: u32 = 0x20;
    pub const IFF_RUNNING: u32 = 0x40;
    pub const IFF_NOARP: u32 = 0x80;
    pub const IFF_PROMISC: u32 = 0x100;
    pub const IFF_ALLMULTI: u32 = 0x200;
    pub const IFF_MASTER: u32 = 0x400;
    pub const IFF_SLAVE: u32 = 0x800;
    pub const IFF_MULTICAST: u32 = 0x1000;
    pub const IFF_PORTSEL: u32 = 0x2000;
    pub const IFF_AUTOMEDIA: u32 = 0x4000;
    pub const IFF_DYNAMIC: u32 = 0x8000;
    pub const IFF_LOWER_UP: u32 = 0x10000;
    pub const IFF_DORMANT: u32 = 0x20000;
    pub const IFF_ECHO: u32 = 0x40000;
    /// The device's own state, which no request changes (`IFF_VOLATILE`).
    pub const IFF_VOLATILE: u32 = IFF_LOOPBACK
        | IFF_POINTOPOINT
        | IFF_BROADCAST
        | IFF_ECHO
        | IFF_MASTER
        | IFF_SLAVE
        | IFF_RUNNING
        | IFF_LOWER_UP
        | IFF_DORMANT;
    /// What a request sets as it asks (`__dev_change_flags`).
    pub const IFF_SETTABLE: u32 = IFF_DEBUG
        | IFF_NOTRAILERS
        | IFF_NOARP
        | IFF_DYNAMIC
        | IFF_MULTICAST
        | IFF_PORTSEL
        | IFF_AUTOMEDIA;
}

/// `IFA_F_*`, an address's flags (`include/uapi/linux/if_addr.h`).
pub mod ifa {
    pub const IFA_F_SECONDARY: u32 = 0x01;
    pub const IFA_F_NODAD: u32 = 0x02;
    pub const IFA_F_OPTIMISTIC: u32 = 0x04;
    pub const IFA_F_DADFAILED: u32 = 0x08;
    pub const IFA_F_HOMEADDRESS: u32 = 0x10;
    pub const IFA_F_DEPRECATED: u32 = 0x20;
    pub const IFA_F_TENTATIVE: u32 = 0x40;
    pub const IFA_F_PERMANENT: u32 = 0x80;
    pub const IFA_F_MANAGETEMPADDR: u32 = 0x100;
    pub const IFA_F_NOPREFIXROUTE: u32 = 0x200;
    pub const IFA_F_MCAUTOJOIN: u32 = 0x400;
    pub const IFA_F_STABLE_PRIVACY: u32 = 0x800;
    /// What a request may set on an IPv6 address (`IFA_F_*` that `inet6_rtm_newaddr` keeps).
    pub const IPV6_SETTABLE: u32 = IFA_F_NODAD
        | IFA_F_OPTIMISTIC
        | IFA_F_HOMEADDRESS
        | IFA_F_MANAGETEMPADDR
        | IFA_F_NOPREFIXROUTE
        | IFA_F_MCAUTOJOIN;
    /// What `__inet_insert_ifa` drops from an IPv4 address (`IPV6ONLY_FLAGS`).
    pub const IPV6_ONLY: u32 = IFA_F_NODAD
        | IFA_F_OPTIMISTIC
        | IFA_F_DADFAILED
        | IFA_F_HOMEADDRESS
        | IFA_F_TENTATIVE
        | IFA_F_MANAGETEMPADDR
        | IFA_F_STABLE_PRIVACY;
}

/// Route tables, types, protocols and scopes (`include/uapi/linux/rtnetlink.h`).
pub mod rt {
    pub const RT_TABLE_UNSPEC: u32 = 0;
    pub const RT_TABLE_DEFAULT: u32 = 253;
    pub const RT_TABLE_MAIN: u32 = 254;
    pub const RT_TABLE_LOCAL: u32 = 255;
    pub const RTN_UNSPEC: u8 = 0;
    pub const RTN_UNICAST: u8 = 1;
    pub const RTN_LOCAL: u8 = 2;
    pub const RTN_BROADCAST: u8 = 3;
    pub const RTN_BLACKHOLE: u8 = 6;
    pub const RTN_UNREACHABLE: u8 = 7;
    pub const RTN_PROHIBIT: u8 = 8;
    pub const RTN_THROW: u8 = 9;
    pub const RTPROT_UNSPEC: u8 = 0;
    pub const RTPROT_KERNEL: u8 = 2;
    pub const RTPROT_BOOT: u8 = 3;
    pub const RT_SCOPE_UNIVERSE: u8 = 0;
    pub const RT_SCOPE_SITE: u8 = 200;
    pub const RT_SCOPE_LINK: u8 = 253;
    pub const RT_SCOPE_HOST: u8 = 254;
    pub const RT_SCOPE_NOWHERE: u8 = 255;
    /// The metric of an IPv6 route that names none (`IP6_RT_PRIO_USER`), and of the prefix
    /// route of an address (`IP6_RT_PRIO_ADDRCONF`).
    pub const IP6_RT_PRIO_USER: u32 = 1024;
    pub const IP6_RT_PRIO_ADDRCONF: u32 = 256;
}

use flags::*;
use ifa::*;
use rt::*;

/// `IFNAMSIZ`: a name is at most 15 bytes.
pub const IFNAMSIZ: usize = 16;
pub const LOOPBACK_INDEX: u32 = 1;
const ETH_MIN_MTU: u32 = 68;
const ETH_DATA_LEN: u32 = 1500;
const ETH_MAX_MTU: u32 = 65535;

/// What an interface is, which decides where what it sends goes.
#[derive(Clone, Debug)]
pub enum LinkKind {
    /// `lo`.
    Loopback,
    /// The host-facing interface: what it sends leaves through the namespace's backend. It
    /// cannot be deleted or moved to another namespace, and changing it changes only what the
    /// guest sees: nothing reaches the host's own interfaces.
    Uplink,
    /// `dummy` (`drivers/net/dummy.c`): whatever it is given is dropped.
    Dummy,
    /// One end of a `veth` pair (`drivers/net/veth.c`).
    Veth(VethEnd),
}

impl LinkKind {
    /// `rtnl_link_ops->kind`, the `IFLA_INFO_KIND` that made the interface; `None` for those no
    /// request makes, which no request deletes either.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Self::Dummy => Some("dummy"),
            Self::Veth(_) => Some("veth"),
            Self::Loopback | Self::Uplink => None,
        }
    }

    /// `NETIF_F_NETNS_LOCAL`: the interface stays in its namespace.
    pub fn netns_local(&self) -> bool {
        matches!(self, Self::Loopback | Self::Uplink)
    }

    /// `min_mtu` and `max_mtu`; a maximum of 0 is no bound (`dev_validate_mtu`).
    fn mtu_range(&self) -> (u32, u32) {
        match self {
            // `gen_lo_setup` and `dummy_setup` leave both at 0.
            Self::Loopback | Self::Dummy => (0, 0),
            Self::Uplink => (ETH_MIN_MTU, ETH_DATA_LEN),
            Self::Veth(_) => (ETH_MIN_MTU, ETH_MAX_MTU),
        }
    }

    /// `dev_validate_mtu`.
    pub(crate) fn check_mtu(&self, mtu: u32) -> NetResult<()> {
        let (min, max) = self.mtu_range();
        match mtu > i32::MAX as u32 || mtu < min || (max > 0 && mtu > max) {
            true => Err(Errno::EINVAL),
            false => Ok(()),
        }
    }

    /// The flags a new interface of the kind starts with (`ether_setup`, `dummy_setup`,
    /// `gen_lo_setup`).
    pub(crate) fn initial_flags(&self) -> u32 {
        match self {
            Self::Loopback => IFF_LOOPBACK,
            Self::Dummy => IFF_BROADCAST | IFF_NOARP,
            Self::Uplink | Self::Veth(_) => IFF_BROADCAST | IFF_MULTICAST,
        }
    }
}

/// A `veth` pair: two interfaces, each the other's peer, in one namespace or two. What one end
/// sends, the other receives; its carrier is on while both ends are up (`veth_open`).
#[derive(Debug)]
struct VethPair {
    ends: Mutex<[VethSide; 2]>,
}

#[derive(Clone, Debug)]
struct VethSide {
    net: Weak<Net>,
    index: u32,
    up: bool,
}

/// One end of a `veth` pair.
#[derive(Clone, Debug)]
pub struct VethEnd {
    pair: Arc<VethPair>,
    side: usize,
}

impl VethEnd {
    /// A new pair with both ends in `net`, as interfaces `index` and `peer`, both down.
    pub(crate) fn pair(net: Weak<Net>, index: u32, peer: u32) -> (Self, Self) {
        let side = |index| VethSide {
            net: net.clone(),
            index,
            up: false,
        };
        let pair = Arc::new(VethPair {
            ends: Mutex::new([side(index), side(peer)]),
        });
        (
            Self {
                pair: pair.clone(),
                side: 0,
            },
            Self { pair, side: 1 },
        )
    }

    /// The peer's namespace and interface index.
    pub fn peer(&self) -> (Weak<Net>, u32) {
        let ends = self.pair.ends.lock_or_recover();
        let peer = &ends[1 - self.side];
        (peer.net.clone(), peer.index)
    }

    /// Both ends are up, and the peer's namespace is alive.
    pub fn carrier(&self) -> bool {
        let ends = self.pair.ends.lock_or_recover();
        let peer = &ends[1 - self.side];
        ends[self.side].up && peer.up && peer.net.strong_count() > 0
    }

    /// The peer's namespace is gone, which takes this end with it, as Linux deletes both ends
    /// of a pair when one namespace exits.
    pub(crate) fn orphaned(&self) -> bool {
        self.pair.ends.lock_or_recover()[1 - self.side]
            .net
            .strong_count()
            == 0
    }

    fn set_up(&self, up: bool) {
        self.pair.ends.lock_or_recover()[self.side].up = up;
    }

    /// The end now is interface `index` of `net`.
    pub(crate) fn moved(&self, net: Weak<Net>, index: u32) {
        let mut ends = self.pair.ends.lock_or_recover();
        ends[self.side].net = net;
        ends[self.side].index = index;
    }

    /// The other end of the same pair.
    pub(crate) fn is_peer_of(&self, other: &VethEnd) -> bool {
        Arc::ptr_eq(&self.pair, &other.pair) && self.side != other.side
    }
}

/// An address of an interface (`struct in_ifaddr`, `struct inet6_ifaddr`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfAddr {
    /// The local address (`IFA_LOCAL`; IPv6's `IFA_ADDRESS`).
    pub addr: IpAddr,
    pub prefix_len: u8,
    /// The other end of a point-to-point link, IPv4's `IFA_ADDRESS` when it differs.
    pub peer: Option<IpAddr>,
    /// IPv4's `IFA_BROADCAST`, as given.
    pub broadcast: Option<Ipv4Addr>,
    /// `RT_SCOPE_*`.
    pub scope: u8,
    /// `IFA_F_*`.
    pub flags: u32,
    /// IPv4's `IFA_LABEL` when it is an alias (`eth0:1`); `None` is the interface's name.
    pub label: Option<String>,
    /// The metric of the address's prefix route (`IFA_RT_PRIORITY`).
    pub metric: u32,
}

impl IfAddr {
    /// `addr/prefix_len` with Linux's defaults: the scope `ip addr add` gives it, permanent.
    pub fn new(addr: IpAddr, prefix_len: u8) -> Self {
        Self {
            addr,
            prefix_len,
            peer: None,
            broadcast: None,
            scope: default_scope(addr),
            flags: IFA_F_PERMANENT,
            label: None,
            metric: 0,
        }
    }

    /// The IPv4 broadcast address of the subnet, as `ip addr add ... brd +` computes it.
    pub fn subnet_broadcast(&self) -> Option<Ipv4Addr> {
        match self.prefix_addr() {
            IpAddr::V4(a) if self.prefix_len < 31 => Some(Ipv4Addr::from(
                u32::from(a) | !prefix_mask_v4(self.prefix_len),
            )),
            _ => None,
        }
    }

    pub fn is_secondary(&self) -> bool {
        self.flags & IFA_F_SECONDARY != 0
    }

    /// `ifa_address`: what the prefix is of.
    fn prefix_addr(&self) -> IpAddr {
        self.peer.unwrap_or(self.addr)
    }

    /// The same mask and subnet as `other` (`ifa_mask` and `inet_ifa_match`).
    fn same_subnet(&self, other: &IfAddr) -> bool {
        self.prefix_len == other.prefix_len
            && in_prefix(other.prefix_addr(), self.prefix_addr(), self.prefix_len)
    }
}

/// The scope Linux gives an address that names none: host for loopback, link for IPv6
/// link-local (`ipv6_addr_scope`), universe otherwise.
pub fn default_scope(addr: IpAddr) -> u8 {
    match addr {
        IpAddr::V4(a) if a.is_loopback() => RT_SCOPE_HOST,
        IpAddr::V6(a) if a.is_loopback() => RT_SCOPE_HOST,
        IpAddr::V6(a) if a.is_unicast_link_local() => RT_SCOPE_LINK,
        _ => RT_SCOPE_UNIVERSE,
    }
}

#[derive(Clone, Debug)]
pub struct Link {
    /// `ifindex`; loopback is 1, as on Linux.
    pub index: u32,
    pub name: String,
    /// `IFF_*`; `IFF_UP` is administrative state (`ip link set up`). A veth's carrier is its
    /// pair's: [`Link::flags`] has the flags as Linux reports them.
    pub flags: u32,
    pub mtu: u32,
    pub mac: [u8; 6],
    pub kind: LinkKind,
    /// IPv4 addresses in Linux's order (primaries by scope, then secondaries), then IPv6.
    pub addrs: Vec<IfAddr>,
    /// `net.ipv6.conf.<name>.disable_ipv6`: no IPv6 address, not even link-local.
    pub ipv6_disabled: bool,
    /// The multicast groups sockets joined on the interface (`in_device->mc_list`,
    /// `inet6_dev->mc_list`), newest first as `ip_mc_inc_group` and `__ipv6_dev_mc_inc` link
    /// them, with how many memberships hold each. The kernel's own (see
    /// [`Link::groups`]) are not kept here.
    pub joined: Vec<(IpAddr, u32)>,
}

impl Link {
    pub fn is_up(&self) -> bool {
        self.flags & IFF_UP != 0
    }

    pub fn is_loopback(&self) -> bool {
        self.flags & IFF_LOOPBACK != 0
    }

    /// `dev_get_flags`: `IFF_RUNNING` and `IFF_LOWER_UP` follow the carrier.
    pub fn flags(&self) -> u32 {
        match &self.kind {
            LinkKind::Veth(end) => {
                let flags = self.flags & !(IFF_RUNNING | IFF_LOWER_UP);
                match self.is_up() && end.carrier() {
                    true => flags | IFF_RUNNING | IFF_LOWER_UP,
                    false => flags,
                }
            }
            _ => self.flags,
        }
    }

    pub fn carrier(&self) -> bool {
        self.flags() & IFF_LOWER_UP != 0
    }

    /// The groups the kernel joins by itself while the interface is up: IPv4's all-hosts
    /// group (`ip_mc_up`), and with IPv6 the all-nodes groups (`ipv6_mc_up`, interface-local
    /// and link-local) and each address's solicited-node group (`addrconf_join_solict`, not
    /// on loopback), oldest first.
    fn own_groups(&self) -> Vec<IpAddr> {
        if !self.is_up() {
            return Vec::new();
        }
        let mut groups = vec![IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1))];
        if self.ipv6_disabled {
            return groups;
        }
        groups.push(Ipv6Addr::new(0xff01, 0, 0, 0, 0, 0, 0, 1).into());
        groups.push(Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1).into());
        if !self.is_loopback() {
            for a in &self.addrs {
                if let IpAddr::V6(v6) = a.addr {
                    let low = u128::from(v6) & 0xff_ffff;
                    let group = IpAddr::V6(Ipv6Addr::from(SOLICITED_NODE | low));
                    if !groups.contains(&group) {
                        groups.push(group);
                    }
                }
            }
        }
        groups
    }

    /// Every group the interface is in, newest first, with its number of memberships: the
    /// sockets' (`joined`) and the kernel's own, which count as one each (what
    /// `/proc/net/igmp` and `/proc/net/igmp6` list).
    pub fn groups(&self) -> Vec<(IpAddr, u32)> {
        let mut groups = self.joined.clone();
        for own in self.own_groups().into_iter().rev() {
            match groups.iter_mut().find(|(g, _)| *g == own) {
                Some((_, users)) => *users += 1,
                None => groups.push((own, 1)),
            }
        }
        groups
    }

    /// `ip_check_mc_rcu`, `ipv6_chk_mcast_addr`: the interface receives what is sent to
    /// `group`.
    pub fn in_group(&self, group: IpAddr) -> bool {
        self.joined.iter().any(|(g, _)| *g == group) || self.own_groups().contains(&group)
    }
}

/// `ff02::1:ff00:0/104`, the solicited-node prefix.
const SOLICITED_NODE: u128 = 0xff02_0000_0000_0000_0000_0001_ff00_0000;

/// A route (`struct fib_alias` with its `fib_info`, `struct fib6_info`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// `RT_TABLE_*` or a user table.
    pub table: u32,
    pub dst: IpAddr,
    pub prefix_len: u8,
    pub gateway: Option<IpAddr>,
    /// Output interface index; 0 for a route that goes nowhere (`RTN_UNREACHABLE`, ...).
    pub oif: u32,
    /// Preferred source address (`src` in `ip route`).
    pub prefsrc: Option<IpAddr>,
    pub metric: u32,
    /// `RTPROT_*`.
    pub protocol: u8,
    /// `RT_SCOPE_*`.
    pub scope: u8,
    /// `RTN_*`.
    pub kind: u8,
}

impl Route {
    /// A unicast route in the main table, as `ip route add` makes one: scope link without a
    /// gateway, universe with one.
    pub fn unicast(dst: IpAddr, prefix_len: u8, gateway: Option<IpAddr>, oif: u32) -> Self {
        Self {
            table: RT_TABLE_MAIN,
            dst,
            prefix_len,
            gateway,
            oif,
            prefsrc: None,
            metric: 0,
            protocol: RTPROT_BOOT,
            scope: match gateway {
                Some(_) => RT_SCOPE_UNIVERSE,
                None => RT_SCOPE_LINK,
            },
            kind: RTN_UNICAST,
        }
    }

    /// The error a lookup that ends here fails with (`fib_props[type].error`); `None` for a
    /// route that delivers.
    fn error(&self) -> Option<Errno> {
        match self.kind {
            RTN_UNREACHABLE => Some(Errno::EHOSTUNREACH),
            RTN_BLACKHOLE => Some(Errno::EINVAL),
            RTN_PROHIBIT => Some(Errno::EACCES),
            RTN_THROW => Some(Errno::EAGAIN),
            _ => None,
        }
    }

    /// The prefix route Linux makes for an address (`fib_add_ifaddr`,
    /// `addrconf_prefix_route`).
    fn prefix_of(a: &IfAddr, oif: u32) -> Self {
        let v4 = a.addr.is_ipv4();
        Self {
            table: RT_TABLE_MAIN,
            dst: network_of(a.prefix_addr(), a.prefix_len),
            prefix_len: a.prefix_len,
            gateway: None,
            oif,
            prefsrc: v4.then_some(a.addr),
            metric: match (v4, a.metric) {
                (false, 0) => IP6_RT_PRIO_ADDRCONF,
                (_, m) => m,
            },
            protocol: RTPROT_KERNEL,
            scope: match v4 {
                true => RT_SCOPE_LINK,
                false => RT_SCOPE_UNIVERSE,
            },
            kind: RTN_UNICAST,
        }
    }
}

/// How an add treats an entry that exists (`NLM_F_CREATE`, `NLM_F_EXCL`, `NLM_F_REPLACE`,
/// `NLM_F_APPEND`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddMode {
    pub create: bool,
    pub excl: bool,
    pub replace: bool,
    pub append: bool,
}

impl AddMode {
    /// `ip ... add`: create, fail if it exists.
    pub const ADD: Self = Self {
        create: true,
        excl: true,
        replace: false,
        append: false,
    };
}

/// Which address a delete takes (`inet_rtm_deladdr`, `inet6_rtm_deladdr`): the first of the
/// family that matches every field given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddrMatch {
    pub v6: bool,
    /// `IFA_LOCAL`.
    pub local: Option<IpAddr>,
    /// `IFA_ADDRESS`, compared by prefix (IPv4) or as the address (IPv6).
    pub address: Option<IpAddr>,
    pub prefix_len: u8,
    pub label: Option<String>,
}

/// Which route a delete takes: the first that matches every field given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteMatch {
    pub table: u32,
    pub dst: IpAddr,
    pub prefix_len: u8,
    pub gateway: Option<IpAddr>,
    pub oif: Option<u32>,
    pub prefsrc: Option<IpAddr>,
    pub metric: Option<u32>,
    pub protocol: Option<u8>,
    pub scope: Option<u8>,
    pub kind: Option<u8>,
}

/// Where a lookup sends a destination.
#[derive(Clone, Debug)]
pub(crate) enum Via {
    /// An address of the namespace.
    Local,
    /// The uplink, to the backend.
    Uplink,
    /// A veth end, to the namespace at its other end.
    Veth(VethEnd),
    /// A link where nothing answers: a dummy, or a veth without carrier, where neighbour
    /// resolution fails (`unresolved`; a dummy is `IFF_NOARP` and drops what it is given).
    Nowhere { unresolved: bool },
}

/// What a route lookup decided for a destination.
#[derive(Clone, Debug)]
pub struct RouteDecision {
    pub oif: u32,
    /// Source address a socket bound to the wildcard address uses.
    pub src: IpAddr,
    /// The destination is an address of this namespace.
    pub local: bool,
    /// The route that matched; `None` for a local destination.
    pub route: Option<Route>,
    pub(crate) via: Via,
}

/// The interface and route tables of a namespace.
#[derive(Clone, Debug)]
pub struct Interfaces {
    links: Vec<Link>,
    routes: Vec<Route>,
    /// The last index given out: indexes are handed out cyclically, as `dev_index_reserve`
    /// does, so a deleted interface's index is not reused at once.
    last_index: u32,
}

fn prefix_mask_v4(len: u8) -> u32 {
    if len == 0 {
        0
    } else {
        u32::MAX << (32 - len.min(32))
    }
}

fn prefix_mask_v6(len: u8) -> u128 {
    if len == 0 {
        0
    } else {
        u128::MAX << (128 - len.min(128))
    }
}

/// Is `ip` inside `net/len`?
pub fn in_prefix(ip: IpAddr, net: IpAddr, len: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => {
            let m = prefix_mask_v4(len);
            u32::from(a) & m == u32::from(n) & m
        }
        (IpAddr::V6(a), IpAddr::V6(n)) => {
            let m = prefix_mask_v6(len);
            u128::from(a) & m == u128::from(n) & m
        }
        _ => false,
    }
}

/// `addr` with the bits past `len` cleared.
pub fn network_of(addr: IpAddr, len: u8) -> IpAddr {
    match addr {
        IpAddr::V4(a) => Ipv4Addr::from(u32::from(a) & prefix_mask_v4(len)).into(),
        IpAddr::V6(a) => Ipv6Addr::from(u128::from(a) & prefix_mask_v6(len)).into(),
    }
}

fn max_prefix(addr: IpAddr) -> u8 {
    match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

/// `dev_valid_name`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < IFNAMSIZ
        && name != "."
        && name != ".."
        && !name
            .bytes()
            .any(|b| b == b'/' || b == b':' || b == 0 || b.is_ascii_whitespace())
}

/// `is_valid_ether_addr`: neither multicast nor all zeros.
pub fn valid_mac(mac: [u8; 6]) -> bool {
    mac[0] & 1 == 0 && mac != [0; 6]
}

/// The modified EUI-64 link-local address of `mac` (`addrconf_ifid_eui48`).
fn link_local(mac: [u8; 6]) -> Ipv6Addr {
    let [a, b, c, d, e, f] = mac;
    Ipv6Addr::from([
        0xfe,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        a ^ 2,
        b,
        c,
        0xff,
        0xfe,
        d,
        e,
        f,
    ])
}

impl Interfaces {
    /// Loopback only. `lo_up` is true for a machine's first namespace; a namespace made by
    /// `unshare(CLONE_NEWNET)` starts with `lo` down and without addresses, as on Linux.
    pub fn loopback_only(lo_up: bool) -> Self {
        let kind = LinkKind::Loopback;
        let mut this = Self {
            links: vec![Link {
                index: LOOPBACK_INDEX,
                name: "lo".into(),
                flags: kind.initial_flags(),
                mtu: 65536,
                mac: [0; 6],
                kind,
                addrs: Vec::new(),
                ipv6_disabled: false,
                joined: Vec::new(),
            }],
            routes: Vec::new(),
            last_index: LOOPBACK_INDEX,
        };
        if lo_up {
            this.set_up(LOOPBACK_INDEX, true).expect("lo exists");
        }
        this
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    pub fn link(&self, index: u32) -> Option<&Link> {
        self.links.iter().find(|l| l.index == index)
    }

    fn link_mut(&mut self, index: u32) -> NetResult<&mut Link> {
        self.links
            .iter_mut()
            .find(|l| l.index == index)
            .ok_or(Errno::ENODEV)
    }

    pub fn link_by_name(&self, name: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.name == name)
    }

    /// `dev_get_valid_name`: `EINVAL` for a name Linux refuses, `EEXIST` for one in use. A name
    /// with `%d` takes the lowest free number (`dev_alloc_name`).
    pub fn check_name(&self, name: &str) -> NetResult<String> {
        if let Some((head, tail)) = name.split_once("%d") {
            if tail.contains('%') || head.contains('%') {
                return Err(Errno::EINVAL);
            }
            return (0u32..32768)
                .map(|n| format!("{head}{n}{tail}"))
                .find(|n| self.link_by_name(n).is_none())
                .filter(|n| valid_name(n))
                .ok_or(Errno::ENFILE);
        }
        if !valid_name(name) {
            return Err(Errno::EINVAL);
        }
        if self.link_by_name(name).is_some() {
            return Err(Errno::EEXIST);
        }
        Ok(name.into())
    }

    /// An index for a new interface: `wanted` if free and given, else the next free one after
    /// the last given out.
    pub(crate) fn reserve_index(&mut self, wanted: Option<u32>) -> u32 {
        if let Some(i) = wanted.filter(|&i| i != 0 && self.link(i).is_none()) {
            return i;
        }
        let mut i = self.last_index;
        loop {
            i = match i.checked_add(1) {
                Some(i) if i <= i32::MAX as u32 => i,
                _ => 1,
            };
            if self.link(i).is_none() {
                self.last_index = i;
                return i;
            }
        }
    }

    /// Adds an interface, down and without addresses, and returns its index.
    pub fn add_link(
        &mut self,
        name: &str,
        kind: LinkKind,
        mtu: u32,
        mac: [u8; 6],
    ) -> NetResult<u32> {
        let name = self.check_name(name)?;
        let index = self.reserve_index(None);
        self.insert(Link {
            index,
            name,
            flags: kind.initial_flags(),
            mtu,
            mac,
            kind,
            addrs: Vec::new(),
            ipv6_disabled: false,
            joined: Vec::new(),
        });
        Ok(index)
    }

    /// Puts a link whose name and index were checked into the table.
    pub(crate) fn insert(&mut self, link: Link) {
        let at = self.links.partition_point(|l| l.index < link.index);
        self.links.insert(at, link);
    }

    /// `unregister_netdevice`: the interface goes down, and its addresses and every route
    /// through it go with it.
    pub(crate) fn remove(&mut self, index: u32) -> NetResult<Link> {
        self.set_up(index, false)?;
        let i = self
            .links
            .iter()
            .position(|l| l.index == index)
            .ok_or(Errno::ENODEV)?;
        let mut link = self.links.remove(i);
        let addrs = std::mem::take(&mut link.addrs);
        self.routes.retain(|r| r.oif != index);
        for a in &addrs {
            self.forget_prefsrc(a.addr);
        }
        Ok(link)
    }

    /// `dev_change_name`: only while down, as Linux renames a running interface only with
    /// `IFF_LIVE_RENAME_OK`. Aliases follow (`inetdev_changename`).
    pub fn rename(&mut self, index: u32, name: &str) -> NetResult<()> {
        let link = self.link(index).ok_or(Errno::ENODEV)?;
        if link.name == name {
            return Ok(());
        }
        if link.is_up() {
            return Err(Errno::EBUSY);
        }
        let name = self.check_name(name)?;
        let link = self.link_mut(index)?;
        let old = std::mem::replace(&mut link.name, name.clone());
        for a in &mut link.addrs {
            if let Some(alias) = a.label.as_mut().and_then(|l| l.strip_prefix(&old)) {
                a.label = Some(format!("{name}{alias}"));
            }
        }
        Ok(())
    }

    /// `dev_set_mtu`: within the device's bounds (`dev_validate_mtu`).
    pub fn set_mtu(&mut self, index: u32, mtu: u32) -> NetResult<()> {
        let link = self.link_mut(index)?;
        link.kind.check_mtu(mtu)?;
        link.mtu = mtu;
        Ok(())
    }

    /// `dev_set_mac_address` with `eth_prepare_mac_addr_change`'s check: every interface here
    /// takes a change while running (`IFF_LIVE_ADDR_CHANGE`).
    pub fn set_mac(&mut self, index: u32, mac: [u8; 6]) -> NetResult<()> {
        let link = self.link_mut(index)?;
        if !valid_mac(mac) {
            return Err(Errno::EADDRNOTAVAIL);
        }
        link.mac = mac;
        Ok(())
    }

    /// `ip link set up/down` (`dev_open`, `dev_close`), with what IPv4 and IPv6 do as the
    /// interface goes up (`NETDEV_UP`) or down (`NETDEV_DOWN`).
    pub fn set_up(&mut self, index: u32, up: bool) -> NetResult<()> {
        let link = self.link_mut(index)?;
        let was = link.is_up();
        let carrier = match &link.kind {
            LinkKind::Veth(end) => {
                end.set_up(up);
                0
            }
            _ => IFF_RUNNING | IFF_LOWER_UP,
        };
        if up {
            link.flags |= IFF_UP | carrier;
        } else {
            link.flags &= !(IFF_UP | IFF_RUNNING | IFF_LOWER_UP);
        }
        match (was, up) {
            (false, true) => self.netdev_up(index),
            (true, false) => self.netdev_down(index),
            _ => {}
        }
        Ok(())
    }

    /// `inetdev_event` and `addrconf_notify` on `NETDEV_UP`: loopback gets 127.0.0.1/8 and
    /// ::1, other interfaces their link-local address; IPv4 addresses get their prefix routes
    /// back.
    fn netdev_up(&mut self, index: u32) {
        let Ok(link) = self.link_mut(index) else {
            return;
        };
        let loopback = link.is_loopback();
        let mut new = Vec::new();
        if loopback {
            new.push(IfAddr::new(Ipv4Addr::LOCALHOST.into(), 8));
        }
        if !link.ipv6_disabled {
            new.push(match loopback {
                true => IfAddr::new(Ipv6Addr::LOCALHOST.into(), 128),
                false => IfAddr::new(link_local(link.mac).into(), 64),
            });
        }
        let primaries: Vec<IfAddr> = link
            .addrs
            .iter()
            .filter(|a| a.addr.is_ipv4() && !a.is_secondary())
            .cloned()
            .collect();
        for a in primaries {
            self.add_prefix_route(&a, index);
        }
        for a in new {
            match self.add_addr(index, a, AddMode::ADD) {
                Ok(()) | Err(Errno::EEXIST) => {}
                Err(e) => tracing::warn!(?e, index, "an address of an interface coming up"),
            }
        }
    }

    /// `fib_disable_ip` and `addrconf_ifdown` on `NETDEV_DOWN`: IPv4 routes through the
    /// interface go (but its addresses stay), IPv6 addresses and routes go
    /// (`keep_addr_on_down` is 0).
    fn netdev_down(&mut self, index: u32) {
        self.routes
            .retain(|r| r.oif != index || (r.dst.is_ipv4() && r.scope == RT_SCOPE_HOST));
        let Ok(link) = self.link_mut(index) else {
            return;
        };
        link.addrs.retain(|a| a.addr.is_ipv4());
    }

    /// `addrconf_disable_ipv6` for one interface: disabling drops its IPv6 addresses and
    /// routes (`addrconf_ifdown`); enabling an up interface gives it its link-local address
    /// (`addrconf_dev_config`).
    pub fn set_ipv6_disabled(&mut self, index: u32, disabled: bool) -> NetResult<()> {
        let link = self.link_mut(index)?;
        if link.ipv6_disabled == disabled {
            return Ok(());
        }
        link.ipv6_disabled = disabled;
        let up = link.is_up();
        if disabled {
            link.addrs.retain(|a| a.addr.is_ipv4());
            self.routes.retain(|r| r.oif != index || r.dst.is_ipv4());
        } else if up {
            self.netdev_up(index);
        }
        Ok(())
    }

    /// `__dev_change_flags`, what `SIOCSIFFLAGS` and `RTM_NEWLINK` do: the flags a request
    /// owns come from `flags`, the device's own state stays, and a change of `IFF_UP` brings
    /// the interface up or down ([`Interfaces::set_up`]). `IFF_PROMISC` and `IFF_ALLMULTI`
    /// follow `flags`, as no one else asks for them here.
    pub fn change_flags(&mut self, index: u32, flags: u32) -> NetResult<()> {
        let link = self.link_mut(index)?;
        let old = link.flags;
        let kept = IFF_UP | IFF_VOLATILE;
        let listening = IFF_PROMISC | IFF_ALLMULTI;
        link.flags = (flags & (IFF_SETTABLE | listening)) | (old & kept);
        if (old ^ flags) & IFF_UP != 0 {
            self.set_up(index, flags & IFF_UP != 0)?;
        }
        Ok(())
    }

    /// `ip addr add` (`inet_rtm_newaddr`, `inet6_rtm_newaddr`). An address that exists (IPv4:
    /// the same local address, prefix length and subnet; IPv6: the same address) is `EEXIST`
    /// unless `mode` replaces it, which changes its prefix route's metric and its flags. An
    /// IPv4 address in the subnet of another is that one's secondary, which has no prefix
    /// route of its own.
    pub fn add_addr(&mut self, index: u32, a: IfAddr, mode: AddMode) -> NetResult<()> {
        if a.prefix_len > max_prefix(a.addr) {
            return Err(Errno::EINVAL);
        }
        let link = self.link_mut(index)?;
        let up = link.is_up();
        let loopback = link.is_loopback();
        let v4 = a.addr.is_ipv4();
        // `addrconf_add_dev`: "IPv6 is disabled on this device".
        if !v4 && link.ipv6_disabled {
            return Err(Errno::EACCES);
        }
        let existing = link.addrs.iter().position(|b| match v4 {
            true => b.addr == a.addr && b.same_subnet(&a),
            false => b.addr == a.addr,
        });
        if let Some(i) = existing {
            if mode.excl || !mode.replace {
                return Err(Errno::EEXIST);
            }
            let old = link.addrs[i].clone();
            let b = &mut link.addrs[i];
            if !v4 {
                b.flags = (b.flags & !IPV6_SETTABLE) | (a.flags & IPV6_SETTABLE);
            }
            b.metric = a.metric;
            let new = b.clone();
            if old.metric != new.metric || old.flags != new.flags {
                self.drop_prefix_route(&old, index);
                if (up || !v4) && !new.is_secondary() {
                    self.add_prefix_route(&new, index);
                }
            }
            return Ok(());
        }
        let mut a = a;
        a.flags |= IFA_F_PERMANENT;
        if v4 {
            a.flags &= !(IFA_F_SECONDARY | IPV6_ONLY);
            // `__inet_insert_ifa`: after the primaries of the same or a wider scope.
            let mut at = 0;
            for (i, b) in link
                .addrs
                .iter()
                .enumerate()
                .filter(|(_, b)| b.addr.is_ipv4())
            {
                if !b.is_secondary() && a.scope <= b.scope {
                    at = i + 1;
                }
                if b.same_subnet(&a) {
                    if b.scope != a.scope {
                        return Err(Errno::EINVAL);
                    }
                    a.flags |= IFA_F_SECONDARY;
                }
            }
            let at = match a.is_secondary() {
                true => link.addrs.iter().take_while(|b| b.addr.is_ipv4()).count(),
                false => at,
            };
            link.addrs.insert(at, a.clone());
            if up && !a.is_secondary() && !loopback {
                self.add_prefix_route(&a, index);
            }
        } else {
            a.scope = default_scope(a.addr);
            a.flags &= IPV6_SETTABLE | IFA_F_PERMANENT;
            a.label = None;
            a.peer = None;
            a.broadcast = None;
            link.addrs.push(a.clone());
            self.add_prefix_route(&a, index);
        }
        Ok(())
    }

    /// The prefix route of address `a` of interface `index`, unless it has none: a /32 or
    /// /128, `IFA_F_NOPREFIXROUTE`, a zero network, an IPv4 address of loopback.
    fn add_prefix_route(&mut self, a: &IfAddr, index: u32) {
        let none = (a.prefix_len == max_prefix(a.addr) && a.peer.is_none())
            || a.flags & IFA_F_NOPREFIXROUTE != 0
            || matches!(a.prefix_addr(), IpAddr::V4(v4) if v4.octets()[0] == 0)
            || (a.addr.is_ipv4() && self.link(index).is_some_and(Link::is_loopback));
        if none {
            return;
        }
        let route = Route::prefix_of(a, index);
        if !self.routes.contains(&route) {
            self.routes.push(route);
        }
    }

    fn drop_prefix_route(&mut self, a: &IfAddr, index: u32) {
        let route = Route::prefix_of(a, index);
        if let Some(i) = self.routes.iter().position(|r| *r == route) {
            self.routes.remove(i);
        }
    }

    /// `fib_sync_down_addr`: once `addr` is no longer an address of the namespace, the routes
    /// that took it as their source go.
    fn forget_prefsrc(&mut self, addr: IpAddr) {
        let still = self
            .links
            .iter()
            .any(|l| l.addrs.iter().any(|a| a.addr == addr));
        if !still {
            self.routes.retain(|r| r.prefsrc != Some(addr));
        }
    }

    /// `ip addr del` (`inet_rtm_deladdr`, `inet6_rtm_deladdr`): the first address that
    /// matches, `EADDRNOTAVAIL` if none does. Deleting an IPv4 primary deletes its
    /// secondaries too (`promote_secondaries` is 0).
    pub fn del_addr(&mut self, index: u32, m: &AddrMatch) -> NetResult<()> {
        let link = self.link_mut(index)?;
        let name = link.name.clone();
        let i = link
            .addrs
            .iter()
            .position(|a| match (m.v6, a.addr) {
                (false, IpAddr::V4(_)) => {
                    m.local.is_none_or(|l| l == a.addr)
                        && m.label
                            .as_ref()
                            .is_none_or(|l| *l == *a.label.as_ref().unwrap_or(&name))
                        && m.address.is_none_or(|p| {
                            m.prefix_len == a.prefix_len
                                && in_prefix(p, a.prefix_addr(), a.prefix_len)
                        })
                }
                (true, IpAddr::V6(_)) => {
                    let want = m.local.or(m.address);
                    want == Some(a.addr) && m.prefix_len == a.prefix_len
                }
                _ => false,
            })
            .ok_or(Errno::EADDRNOTAVAIL)?;
        let gone = link.addrs.remove(i);
        let mut removed = vec![gone.clone()];
        if gone.addr.is_ipv4() && !gone.is_secondary() {
            link.addrs.retain(|b| {
                let secondary = b.addr.is_ipv4() && b.is_secondary() && b.same_subnet(&gone);
                if secondary {
                    removed.push(b.clone());
                }
                !secondary
            });
        }
        for a in &removed {
            self.drop_prefix_route(a, index);
            self.forget_prefsrc(a.addr);
        }
        // `inetaddr_event`: the last IPv4 address gone disables IP on the interface
        // (`fib_disable_ip` with force), and every IPv4 route through it goes.
        let v4_left = self
            .link(index)
            .is_some_and(|l| l.addrs.iter().any(|a| a.addr.is_ipv4()));
        if gone.addr.is_ipv4() && !v4_left {
            self.routes.retain(|r| r.oif != index || r.dst.is_ipv6());
        }
        Ok(())
    }

    /// `ip route add` (`fib_table_insert`, `ip6_route_add`), checked as `fib_create_info` and
    /// `fib_check_nh` check it: a gateway must be reachable through a directly connected
    /// route, and the output interface must be up.
    pub fn add_route(&mut self, route: Route, mode: AddMode) -> NetResult<()> {
        let route = self.check_route(route)?;
        let v4 = route.dst.is_ipv4();
        let same_key = |r: &Route| {
            r.table == route.table
                && r.dst == route.dst
                && r.prefix_len == route.prefix_len
                && r.metric == route.metric
        };
        let Some(first) = self.routes.iter().position(same_key) else {
            // IPv6 only warns about a missing `NLM_F_CREATE`.
            if v4 && !mode.create {
                return Err(Errno::ENOENT);
            }
            self.routes.push(route);
            return Ok(());
        };
        if mode.excl {
            return Err(Errno::EEXIST);
        }
        if mode.replace {
            self.routes[first] = route;
            return Ok(());
        }
        let duplicate = self.routes.iter().any(|r| {
            same_key(r)
                && r.kind == route.kind
                && r.oif == route.oif
                && r.gateway == route.gateway
                && (!v4 || (r.scope == route.scope && r.prefsrc == route.prefsrc))
        });
        if duplicate {
            return Err(Errno::EEXIST);
        }
        if !v4 && !mode.create {
            return Err(Errno::ENOENT);
        }
        let at = match mode.append {
            true => self
                .routes
                .iter()
                .rposition(same_key)
                .map_or(first, |i| i + 1),
            false => first,
        };
        self.routes.insert(at, route);
        Ok(())
    }

    /// Validates a new route and fills in what Linux derives: the gateway's interface, IPv6's
    /// default metric and scope.
    fn check_route(&self, mut r: Route) -> NetResult<Route> {
        let v4 = r.dst.is_ipv4();
        if r.prefix_len > max_prefix(r.dst)
            || r.gateway.is_some_and(|g| g.is_ipv4() != v4)
            || r.prefsrc.is_some_and(|s| s.is_ipv4() != v4)
        {
            return Err(Errno::EINVAL);
        }
        if v4 {
            // `fib_table_insert`: "Invalid prefix for given prefix length".
            if network_of(r.dst, r.prefix_len) != r.dst || r.scope > RT_SCOPE_HOST {
                return Err(Errno::EINVAL);
            }
        } else {
            r.dst = network_of(r.dst, r.prefix_len);
            r.scope = RT_SCOPE_UNIVERSE;
            if r.metric == 0 {
                r.metric = IP6_RT_PRIO_USER;
            }
        }
        if r.table == RT_TABLE_UNSPEC {
            r.table = RT_TABLE_MAIN;
        }
        match r.kind {
            RTN_UNICAST => {}
            RTN_UNREACHABLE | RTN_BLACKHOLE | RTN_PROHIBIT | RTN_THROW => {
                if r.gateway.is_some() || r.oif != 0 {
                    return Err(Errno::EINVAL);
                }
                return Ok(r);
            }
            _ => return Err(Errno::EOPNOTSUPP),
        }
        match r.gateway {
            Some(_) if v4 && r.scope == RT_SCOPE_HOST => return Err(Errno::EINVAL),
            Some(gw) => r.oif = self.gateway_link(gw, r.oif)?,
            None => {
                let link = self.link(r.oif).ok_or(Errno::ENODEV)?;
                if !link.is_up() {
                    return Err(Errno::ENETDOWN);
                }
            }
        }
        if let Some(src) = r.prefsrc
            && !self.has_addr(src)
        {
            return Err(Errno::EINVAL);
        }
        Ok(r)
    }

    /// `fib_check_nh_v4_gw` and `ip6_validate_gw`: the interface a gateway is reached on, which
    /// a directly connected route (or, for IPv6 link-local gateways, the given interface)
    /// decides.
    fn gateway_link(&self, gw: IpAddr, oif: u32) -> NetResult<u32> {
        if oif != 0 && self.link(oif).is_none() {
            return Err(Errno::ENODEV);
        }
        let link_local = matches!(gw, IpAddr::V6(v6) if v6.is_unicast_link_local());
        let found = if link_local {
            match oif {
                0 => return Err(Errno::EINVAL),
                oif => Some(oif),
            }
        } else if let Some(local) = self
            .links
            .iter()
            .find(|l| l.addrs.iter().any(|a| a.addr == gw))
        {
            if gw.is_ipv6() {
                return Err(Errno::EINVAL);
            }
            Some(local.index)
        } else {
            self.routes
                .iter()
                .filter(|r| {
                    r.kind == RTN_UNICAST
                        && r.gateway.is_none()
                        && (r.table == RT_TABLE_MAIN || r.table == RT_TABLE_DEFAULT)
                        && (oif == 0 || r.oif == oif)
                        && in_prefix(gw, r.dst, r.prefix_len)
                })
                .max_by_key(|r| r.prefix_len)
                .map(|r| r.oif)
        };
        let index = found.ok_or(match gw {
            IpAddr::V4(_) => Errno::ENETUNREACH,
            IpAddr::V6(_) => Errno::EHOSTUNREACH,
        })?;
        match self.link(index) {
            Some(l) if l.is_up() => Ok(index),
            Some(_) => Err(Errno::ENETDOWN),
            None => Err(Errno::ENODEV),
        }
    }

    /// `ip route del` (`fib_table_delete`, `ip6_route_del`): the first route that matches,
    /// `ESRCH` if none does.
    pub fn del_route(&mut self, m: &RouteMatch) -> NetResult<()> {
        let v4 = m.dst.is_ipv4();
        if m.prefix_len > max_prefix(m.dst) {
            return Err(Errno::EINVAL);
        }
        let dst = match v4 {
            true if network_of(m.dst, m.prefix_len) != m.dst => return Err(Errno::EINVAL),
            true => m.dst,
            false => network_of(m.dst, m.prefix_len),
        };
        let table = match m.table {
            RT_TABLE_UNSPEC => RT_TABLE_MAIN,
            t => t,
        };
        let i = self
            .routes
            .iter()
            .position(|r| {
                r.table == table
                    && r.dst == dst
                    && r.prefix_len == m.prefix_len
                    && m.kind.is_none_or(|k| k == RTN_UNSPEC || k == r.kind)
                    && m.scope
                        .is_none_or(|s| s == RT_SCOPE_NOWHERE || s == r.scope)
                    && m.protocol
                        .is_none_or(|p| p == RTPROT_UNSPEC || p == r.protocol)
                    && m.prefsrc.is_none_or(|s| r.prefsrc == Some(s))
                    && m.oif.is_none_or(|o| o == 0 || o == r.oif)
                    && m.gateway.is_none_or(|g| r.gateway == Some(g))
                    && m.metric.is_none_or(|p| p == 0 || p == r.metric)
            })
            .ok_or(Errno::ESRCH)?;
        self.routes.remove(i);
        Ok(())
    }

    fn has_addr(&self, ip: IpAddr) -> bool {
        self.links
            .iter()
            .any(|l| l.addrs.iter().any(|a| a.addr == ip))
    }

    /// The interface that holds `ip`, if it is an address of an up interface. Every address
    /// in a subnet of loopback's is local while `lo` is up, as Linux's local route for the
    /// prefix makes it.
    pub fn local_link(&self, ip: IpAddr) -> Option<u32> {
        self.links.iter().filter(|l| l.is_up()).find_map(|l| {
            l.addrs
                .iter()
                .any(|a| {
                    a.addr == ip
                        || (l.is_loopback() && ip.is_ipv4() && in_prefix(ip, a.addr, a.prefix_len))
                })
                .then_some(l.index)
        })
    }

    pub fn is_local(&self, ip: IpAddr) -> bool {
        self.local_link(ip).is_some()
    }

    /// `__ip_dev_find` and `ipv6_chk_addr` without the interface's state: the interface that
    /// holds `ip`, up or down.
    pub(crate) fn link_with_addr(&self, ip: IpAddr) -> Option<u32> {
        self.links
            .iter()
            .find(|l| l.addrs.iter().any(|a| a.addr == ip))
            .map(|l| l.index)
    }

    /// A membership of `group` on interface `index` (`ip_mc_inc_group`,
    /// `__ipv6_dev_mc_inc`): the first makes the interface join, at the head of its list.
    pub(crate) fn join_group(&mut self, index: u32, group: IpAddr) -> NetResult<()> {
        let link = self.link_mut(index)?;
        match link.joined.iter_mut().find(|(g, _)| *g == group) {
            Some((_, users)) => *users += 1,
            None => link.joined.insert(0, (group, 1)),
        }
        Ok(())
    }

    /// The end of a membership (`ip_mc_dec_group`, `__ipv6_dev_mc_dec`): the last takes the
    /// interface out of the group. An interface that is gone has left every group already.
    pub(crate) fn leave_group(&mut self, index: u32, group: IpAddr) {
        let Ok(link) = self.link_mut(index) else {
            return;
        };
        if let Some(i) = link.joined.iter().position(|(g, _)| *g == group) {
            link.joined[i].1 -= 1;
            if link.joined[i].1 == 0 {
                link.joined.remove(i);
            }
        }
    }

    /// The source address of a multicast datagram out interface `index`
    /// (`inet_select_addr(dev, 0, scope)`, a simplified `ipv6_dev_get_saddr`): IPv4, the
    /// interface's first primary address, else one of another up interface that is not
    /// link-scoped; IPv6, an address of the interface of the group's scope, else any of its
    /// own. `None` if there is none, as Linux then sends from the unspecified address.
    pub(crate) fn mc_source(&self, index: u32, group: IpAddr) -> Option<IpAddr> {
        let link = self.link(index)?;
        match group {
            IpAddr::V4(_) => {
                let primary = |l: &Link| {
                    l.addrs
                        .iter()
                        .find(|a| a.addr.is_ipv4() && !a.is_secondary())
                        .cloned()
                };
                primary(link).map(|a| a.addr).or_else(|| {
                    self.links
                        .iter()
                        .filter(|l| l.is_up() && l.index != index)
                        .filter_map(primary)
                        .find(|a| a.scope != RT_SCOPE_LINK)
                        .map(|a| a.addr)
                })
            }
            IpAddr::V6(g) => {
                let link_scoped = g.segments()[0] & 0x000f <= 2;
                let v6: Vec<&IfAddr> = link.addrs.iter().filter(|a| a.addr.is_ipv6()).collect();
                v6.iter()
                    .find(|a| (a.scope == RT_SCOPE_LINK) == link_scoped)
                    .or(v6.first())
                    .map(|a| a.addr)
            }
        }
    }

    /// An address of interface `index` to answer `peer` from (`inet_select_addr(dev, peer,
    /// scope)`, a simplified `ipv6_dev_get_saddr`): IPv4, a primary address in `peer`'s
    /// subnet, else the first primary; IPv6, the link-local address for a link-local peer,
    /// else one of wider scope, else any.
    pub(crate) fn source_on(&self, index: u32, peer: IpAddr) -> Option<IpAddr> {
        let link = self.link(index)?;
        match peer {
            IpAddr::V4(_) => {
                let primaries: Vec<&IfAddr> = link
                    .addrs
                    .iter()
                    .filter(|a| a.addr.is_ipv4() && !a.is_secondary())
                    .collect();
                primaries
                    .iter()
                    .find(|a| in_prefix(peer, a.prefix_addr(), a.prefix_len))
                    .or(primaries.first())
                    .map(|a| a.addr)
            }
            IpAddr::V6(p) => {
                let v6: Vec<&IfAddr> = link.addrs.iter().filter(|a| a.addr.is_ipv6()).collect();
                let link_local = p.is_unicast_link_local();
                v6.iter()
                    .find(|a| (a.scope == RT_SCOPE_LINK) == link_local)
                    .or(v6.first())
                    .map(|a| a.addr)
            }
        }
    }

    /// The interface an IPv6 multicast datagram leaves by when nothing names one: the first
    /// up interface with IPv6 but loopback, through the `ff00::/8` route `addrconf` gives each
    /// (`addrconf_add_mroute`), which the tables here leave out.
    pub(crate) fn mc6_default_oif(&self) -> Option<u32> {
        self.links
            .iter()
            .find(|l| l.is_up() && !l.is_loopback() && !l.ipv6_disabled)
            .map(|l| l.index)
    }

    /// `fib_lookup` and `ip6_route_output`: local addresses first, then the main table and
    /// the default one, each by the longest matching prefix and on ties the lowest metric,
    /// over up interfaces. No route is `ENETUNREACH`; an unreachable, blackhole or prohibit
    /// route fails with its error.
    pub fn route(&self, dst: IpAddr) -> NetResult<RouteDecision> {
        if let Some(oif) = self.local_link(dst) {
            return Ok(RouteDecision {
                oif,
                src: dst,
                local: true,
                route: None,
                via: Via::Local,
            });
        }
        for table in [RT_TABLE_MAIN, RT_TABLE_DEFAULT] {
            let mut best: Option<&Route> = None;
            for r in self.routes.iter().filter(|r| {
                r.table == table
                    && in_prefix(dst, r.dst, r.prefix_len)
                    && (r.kind != RTN_UNICAST || self.link(r.oif).is_some_and(Link::is_up))
            }) {
                let better = best.is_none_or(|b| {
                    r.prefix_len > b.prefix_len
                        || (r.prefix_len == b.prefix_len && r.metric < b.metric)
                });
                if better {
                    best = Some(r);
                }
            }
            let Some(r) = best else {
                continue;
            };
            match r.error() {
                Some(Errno::EAGAIN) => continue,
                Some(e) => return Err(e),
                None => {}
            }
            let src = r
                .prefsrc
                .or_else(|| self.select_source(r, dst))
                .ok_or(Errno::EADDRNOTAVAIL)?;
            let link = self.link(r.oif).expect("checked above");
            let via = match &link.kind {
                LinkKind::Loopback => Via::Local,
                LinkKind::Uplink => Via::Uplink,
                LinkKind::Veth(end) if link.carrier() => Via::Veth(end.clone()),
                LinkKind::Veth(_) => Via::Nowhere { unresolved: true },
                LinkKind::Dummy => Via::Nowhere { unresolved: false },
            };
            return Ok(RouteDecision {
                oif: r.oif,
                src,
                local: false,
                route: Some(r.clone()),
                via,
            });
        }
        Err(Errno::ENETUNREACH)
    }

    /// The source address for `dst` through route `r`: `inet_select_addr` for IPv4 (a primary
    /// of the output interface, in the gateway's subnet if any, else one of any interface),
    /// a simplified `ipv6_dev_get_saddr` for IPv6 (the narrowest scope that reaches the
    /// destination, the output interface, the longest common prefix).
    fn select_source(&self, r: &Route, dst: IpAddr) -> Option<IpAddr> {
        let oif = self.link(r.oif)?;
        if dst.is_ipv4() {
            let target = r.gateway;
            let scope = match r.gateway {
                Some(_) => RT_SCOPE_LINK,
                None => RT_SCOPE_HOST,
            };
            let primaries = |l: &'_ Link| {
                l.addrs
                    .iter()
                    .filter(|a| a.addr.is_ipv4() && !a.is_secondary())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let on_oif: Vec<_> = primaries(oif)
                .into_iter()
                .filter(|a| a.scope <= scope)
                .collect();
            let pick = on_oif
                .iter()
                .find(|a| target.is_some_and(|t| in_prefix(t, a.prefix_addr(), a.prefix_len)))
                .or(on_oif.first());
            if let Some(a) = pick {
                return Some(a.addr);
            }
            return self.links.iter().filter(|l| l.is_up()).find_map(|l| {
                primaries(l)
                    .into_iter()
                    .find(|a| a.scope != RT_SCOPE_LINK && a.scope <= scope)
                    .map(|a| a.addr)
            });
        }
        let IpAddr::V6(d) = dst else {
            return None;
        };
        let breadth = |scope: u8| match scope {
            RT_SCOPE_HOST => 0,
            RT_SCOPE_LINK => 1,
            RT_SCOPE_SITE => 2,
            _ => 3,
        };
        let want = breadth(default_scope(dst));
        self.links
            .iter()
            .filter(|l| l.is_up())
            .flat_map(|l| l.addrs.iter().map(move |a| (l.index, a)))
            .filter_map(|(index, a)| match a.addr {
                IpAddr::V6(v6) if a.flags & (IFA_F_TENTATIVE | IFA_F_DADFAILED) == 0 => {
                    Some((index, v6, breadth(a.scope)))
                }
                _ => None,
            })
            .max_by_key(|&(index, v6, b)| {
                let scope = match b >= want {
                    true => (1, -b),
                    false => (0, b),
                };
                let common = (u128::from(v6) ^ u128::from(d)).leading_zeros();
                (v6 == d, scope, index == r.oif, common)
            })
            .map(|(_, v6, _)| v6.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn addr(s: &str) -> IfAddr {
        let (a, len) = s.split_once('/').unwrap();
        IfAddr::new(ip(a), len.parse().unwrap())
    }

    fn eth(t: &mut Interfaces, name: &str) -> u32 {
        let i = t
            .add_link(name, LinkKind::Uplink, 1500, [2, 0, 0, 0, 0, 1])
            .unwrap();
        t.set_up(i, true).unwrap();
        i
    }

    fn decision(t: &Interfaces, dst: &str) -> (u32, IpAddr, bool) {
        let d = t.route(ip(dst)).unwrap();
        (d.oif, d.src, d.local)
    }

    #[test]
    fn fresh_namespace_has_lo_down_without_addresses() {
        let t = Interfaces::loopback_only(false);
        assert!(!t.links()[0].is_up());
        assert!(t.links()[0].addrs.is_empty());
        assert_eq!(t.route(ip("127.0.0.1")).err(), Some(Errno::ENETUNREACH));
    }

    #[test]
    fn changing_flags_brings_lo_up_and_keeps_the_devices_own_state() {
        let mut t = Interfaces::loopback_only(false);
        t.change_flags(LOOPBACK_INDEX, IFF_UP | IFF_NOARP).unwrap();
        let lo = &t.links()[0];
        let want = IFF_UP | IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP | IFF_NOARP;
        assert_eq!(lo.flags, want);
        assert!(t.is_local(ip("127.0.0.1")));
        // Clearing IFF_LOOPBACK or IFF_RUNNING asks for nothing; IFF_UP left out takes it down.
        t.change_flags(LOOPBACK_INDEX, IFF_PROMISC).unwrap();
        let lo = &t.links()[0];
        assert_eq!(lo.flags, IFF_LOOPBACK | IFF_PROMISC);
        assert!(t.route(ip("127.0.0.1")).is_err());
        assert_eq!(t.change_flags(9, IFF_UP), Err(Errno::ENODEV));
    }

    #[test]
    fn loopback_covers_127_slash_8() {
        let t = Interfaces::loopback_only(true);
        assert!(t.is_local(ip("127.0.0.1")));
        assert!(t.is_local(ip("127.1.2.3")));
        assert!(t.is_local(ip("::1")));
        assert!(!t.is_local(ip("10.0.0.1")));
        assert_eq!(t.route(ip("8.8.8.8")).err(), Some(Errno::ENETUNREACH));
    }

    #[test]
    fn longest_prefix_and_source_selection() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        t.add_addr(eth, addr("10.0.2.15/24"), AddMode::ADD).unwrap();
        let mut default = Route::unicast(ip("0.0.0.0"), 0, Some(ip("10.0.2.2")), 0);
        default.metric = 100;
        t.add_route(default, AddMode::ADD).unwrap();
        assert_eq!(decision(&t, "1.1.1.1"), (eth, ip("10.0.2.15"), false));
        assert!(decision(&t, "10.0.2.15").2);
        assert_eq!(
            t.links()[1].addrs[0].subnet_broadcast(),
            Some("10.0.2.255".parse().unwrap())
        );
        t.set_up(eth, false).unwrap();
        assert!(t.route(ip("1.1.1.1")).is_err());
    }

    /// `ip link set down` flushes IPv4 routes through the interface (static ones stay gone)
    /// and IPv6 addresses; `up` brings back the prefix routes and the link-local address.
    #[test]
    fn down_and_up_follow_netdev_events() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        assert_eq!(t.links()[1].addrs[0].addr, ip("fe80::ff:fe00:1"));
        t.add_addr(eth, addr("10.0.0.1/24"), AddMode::ADD).unwrap();
        t.add_addr(eth, addr("fd00::1/64"), AddMode::ADD).unwrap();
        t.add_route(
            Route::unicast(ip("0.0.0.0"), 0, Some(ip("10.0.0.254")), 0),
            AddMode::ADD,
        )
        .unwrap();
        assert_eq!(t.routes().len(), 4);
        t.set_up(eth, false).unwrap();
        assert!(t.routes().is_empty());
        assert_eq!(t.links()[1].addrs, [addr("10.0.0.1/24")]);
        t.set_up(eth, true).unwrap();
        let dsts: Vec<_> = t.routes().iter().map(|r| (r.dst, r.prefix_len)).collect();
        assert_eq!(dsts, [(ip("10.0.0.0"), 24), (ip("fe80::"), 64)]);
    }

    /// Secondaries: a second address in a subnet has no prefix route, and goes with its
    /// primary.
    #[test]
    fn secondaries_go_with_their_primary() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        t.add_addr(eth, addr("10.0.0.1/24"), AddMode::ADD).unwrap();
        t.add_addr(eth, addr("10.0.0.2/24"), AddMode::ADD).unwrap();
        t.add_addr(eth, addr("10.0.0.1/16"), AddMode::ADD).unwrap();
        assert_eq!(
            t.add_addr(eth, addr("10.0.0.2/24"), AddMode::ADD),
            Err(Errno::EEXIST)
        );
        let v4: Vec<_> = t.links()[1]
            .addrs
            .iter()
            .filter(|a| a.addr.is_ipv4())
            .collect();
        assert_eq!(v4.len(), 3);
        assert!(v4[2].is_secondary() && v4[2].addr == ip("10.0.0.2"));
        let del = |local: &str, len| AddrMatch {
            v6: false,
            local: Some(ip(local)),
            address: Some(ip(local)),
            prefix_len: len,
            label: None,
        };
        assert_eq!(
            t.del_addr(eth, &del("10.0.0.3", 24)),
            Err(Errno::EADDRNOTAVAIL)
        );
        t.del_addr(eth, &del("10.0.0.1", 24)).unwrap();
        let left: Vec<_> = t.links()[1]
            .addrs
            .iter()
            .filter(|a| a.addr.is_ipv4())
            .collect();
        assert_eq!(left.len(), 1);
        assert_eq!((left[0].addr, left[0].prefix_len), (ip("10.0.0.1"), 16));
        assert!(t.route(ip("10.0.0.9")).is_ok());
        assert_eq!(t.route(ip("10.1.0.9")).err(), Some(Errno::ENETUNREACH));
        // The last IPv4 address takes every IPv4 route through the interface with it.
        t.add_route(
            Route::unicast(ip("0.0.0.0"), 0, Some(ip("10.0.0.254")), 0),
            AddMode::ADD,
        )
        .unwrap();
        t.del_addr(eth, &del("10.0.0.1", 16)).unwrap();
        assert!(t.routes().iter().all(|r| r.dst.is_ipv6()));
    }

    /// `fib_table_insert`'s flags, and `fib_check_nh`'s checks of a gateway.
    #[test]
    fn route_changes_follow_fib() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        t.add_addr(eth, addr("10.0.0.1/24"), AddMode::ADD).unwrap();
        let via = |gw: &str| Route::unicast(ip("192.168.0.0"), 16, Some(ip(gw)), 0);
        assert_eq!(
            t.add_route(via("172.16.0.1"), AddMode::ADD),
            Err(Errno::ENETUNREACH)
        );
        let mut bad = via("10.0.0.2");
        bad.dst = ip("192.168.1.0");
        assert_eq!(t.add_route(bad, AddMode::ADD), Err(Errno::EINVAL));
        t.add_route(via("10.0.0.2"), AddMode::ADD).unwrap();
        assert_eq!(
            t.add_route(via("10.0.0.3"), AddMode::ADD),
            Err(Errno::EEXIST)
        );
        let replace = AddMode {
            create: true,
            replace: true,
            ..AddMode::default()
        };
        t.add_route(via("10.0.0.3"), replace).unwrap();
        assert_eq!(
            t.route(ip("192.168.4.4")).unwrap().route.unwrap().gateway,
            Some(ip("10.0.0.3"))
        );
        let change = AddMode::default();
        let mut other = via("10.0.0.3");
        other.dst = ip("192.169.0.0");
        assert_eq!(t.add_route(other, change), Err(Errno::ENOENT));
        let m = RouteMatch {
            table: 0,
            dst: ip("192.168.0.0"),
            prefix_len: 16,
            gateway: Some(ip("10.0.0.2")),
            oif: None,
            prefsrc: None,
            metric: None,
            protocol: None,
            scope: None,
            kind: None,
        };
        assert_eq!(t.del_route(&m), Err(Errno::ESRCH));
        t.del_route(&RouteMatch { gateway: None, ..m }).unwrap();
        let mut unreachable = Route::unicast(ip("192.168.0.0"), 16, None, 0);
        unreachable.kind = RTN_UNREACHABLE;
        t.add_route(unreachable, AddMode::ADD).unwrap();
        assert_eq!(t.route(ip("192.168.0.1")).err(), Some(Errno::EHOSTUNREACH));
        let dummy = t
            .add_link("dummy0", LinkKind::Dummy, 1500, [2, 0, 0, 0, 0, 2])
            .unwrap();
        assert_eq!(
            t.add_route(
                Route::unicast(ip("10.9.0.0"), 16, None, dummy),
                AddMode::ADD
            ),
            Err(Errno::ENETDOWN)
        );
    }

    #[test]
    fn ipv6_source_takes_the_scope_and_prefix_of_the_destination() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        t.add_addr(eth, addr("fd00:1::1/64"), AddMode::ADD).unwrap();
        t.add_addr(eth, addr("2001:db8::1/64"), AddMode::ADD)
            .unwrap();
        t.add_route(
            Route::unicast(ip("::"), 0, Some(ip("fd00:1::ff")), 0),
            AddMode::ADD,
        )
        .unwrap();
        assert_eq!(decision(&t, "fd00:1::2").1, ip("fd00:1::1"));
        assert_eq!(decision(&t, "2001:db8:5::1").1, ip("2001:db8::1"));
        assert_eq!(decision(&t, "fe80::7").1, ip("fe80::ff:fe00:1"));
    }

    /// `disable_ipv6` takes the IPv6 addresses and routes away and refuses new ones; turning
    /// it off gives the link its link-local address back.
    #[test]
    fn disabling_ipv6_on_an_interface() {
        let mut t = Interfaces::loopback_only(true);
        let eth = eth(&mut t, "eth0");
        t.add_addr(eth, addr("fd00::1/64"), AddMode::ADD).unwrap();
        t.add_addr(eth, addr("10.0.0.1/24"), AddMode::ADD).unwrap();
        t.set_ipv6_disabled(eth, true).unwrap();
        assert!(t.links()[1].addrs.iter().all(|a| a.addr.is_ipv4()));
        assert!(t.routes().iter().all(|r| r.dst.is_ipv4()));
        assert_eq!(
            t.add_addr(eth, addr("fd00::2/64"), AddMode::ADD),
            Err(Errno::EACCES)
        );
        t.set_ipv6_disabled(eth, false).unwrap();
        assert!(
            t.links()[1]
                .addrs
                .iter()
                .any(|a| a.addr == ip("fe80::ff:fe00:1"))
        );
    }

    #[test]
    fn names_follow_dev_valid_name() {
        let mut t = Interfaces::loopback_only(true);
        for bad in ["", ".", "..", "a/b", "a:b", "a b", "0123456789abcdef"] {
            assert_eq!(t.check_name(bad), Err(Errno::EINVAL), "{bad:?}");
        }
        assert_eq!(t.check_name("lo"), Err(Errno::EEXIST));
        t.add_link("dummy0", LinkKind::Dummy, 1500, [2, 0, 0, 0, 0, 2])
            .unwrap();
        assert_eq!(t.check_name("dummy%d").unwrap(), "dummy1");
        let i = t.link_by_name("dummy0").unwrap().index;
        assert_eq!(t.set_mtu(i, u32::MAX), Err(Errno::EINVAL));
        t.set_mtu(i, 9000).unwrap();
        assert_eq!(t.set_mac(i, [1, 0, 0, 0, 0, 0]), Err(Errno::EADDRNOTAVAIL));
        t.set_up(i, true).unwrap();
        assert_eq!(t.rename(i, "dummy7"), Err(Errno::EBUSY));
    }
}
