//! Interfaces and routes of a namespace. These are plain tables: no frames move through them.
//! The kernel serves them over netlink (`RTM_GETLINK`, `RTM_GETADDR`, `RTM_GETROUTE`) and
//! `SIOCGIF*`; the socket layer uses them to decide which addresses are local and where a
//! destination goes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use wasixv2_abi::Errno;

use crate::NetResult;

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

/// `RT_SCOPE_*` of an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Universe,
    Link,
    Host,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfAddr {
    pub addr: IpAddr,
    pub prefix_len: u8,
    pub scope: Scope,
}

impl IfAddr {
    /// The IPv4 broadcast address of the subnet, as `ip addr` shows with `brd`.
    pub fn broadcast(&self) -> Option<Ipv4Addr> {
        match self.addr {
            IpAddr::V4(a) if self.prefix_len < 31 => Some(Ipv4Addr::from(
                u32::from(a) | !prefix_mask_v4(self.prefix_len),
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// `ifindex`; loopback is 1, as on Linux.
    pub index: u32,
    pub name: String,
    /// `IFF_*`; `IFF_UP` is administrative state (`ip link set up`).
    pub flags: u32,
    pub mtu: u32,
    pub mac: [u8; 6],
    pub addrs: Vec<IfAddr>,
}

impl Link {
    pub fn is_up(&self) -> bool {
        self.flags & flags::IFF_UP != 0
    }
    pub fn is_loopback(&self) -> bool {
        self.flags & flags::IFF_LOOPBACK != 0
    }
}

/// A route in the main table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub dst: IpAddr,
    pub prefix_len: u8,
    pub gateway: Option<IpAddr>,
    /// Output interface index.
    pub oif: u32,
    /// Preferred source address (`src` in `ip route`).
    pub prefsrc: Option<IpAddr>,
    pub metric: u32,
}

/// What a route lookup decided for a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecision {
    pub oif: u32,
    /// Source address a socket bound to the wildcard address uses.
    pub src: IpAddr,
    /// The destination is an address of this namespace.
    pub local: bool,
}

/// The interface and route tables of a namespace.
#[derive(Clone, Debug)]
pub struct Interfaces {
    links: Vec<Link>,
    routes: Vec<Route>,
}

pub const LOOPBACK_INDEX: u32 = 1;

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

impl Interfaces {
    /// Loopback only. `lo_up` is true for a machine's first namespace; a namespace made by
    /// `unshare(CLONE_NEWNET)` starts with `lo` down and without addresses, as on Linux.
    pub fn loopback_only(lo_up: bool) -> Self {
        let mut this = Self {
            links: vec![Link {
                index: LOOPBACK_INDEX,
                name: "lo".into(),
                flags: flags::IFF_LOOPBACK,
                mtu: 65536,
                mac: [0; 6],
                addrs: Vec::new(),
            }],
            routes: Vec::new(),
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

    pub fn link_by_name(&self, name: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.name == name)
    }

    /// Adds an interface (down, without addresses) and returns its index.
    pub fn add_link(&mut self, name: &str, mtu: u32, mac: [u8; 6]) -> u32 {
        let index = self.links.iter().map(|l| l.index).max().unwrap_or(0) + 1;
        self.links.push(Link {
            index,
            name: name.into(),
            flags: flags::IFF_BROADCAST | flags::IFF_MULTICAST,
            mtu,
            mac,
            addrs: Vec::new(),
        });
        index
    }

    /// `ip link set up/down`. Bringing `lo` up assigns 127.0.0.1/8 and ::1/128, as the Linux
    /// loopback driver does.
    pub fn set_up(&mut self, index: u32, up: bool) -> NetResult<()> {
        let link = self
            .links
            .iter_mut()
            .find(|l| l.index == index)
            .ok_or(Errno::ENODEV)?;
        if up {
            link.flags |= flags::IFF_UP | flags::IFF_RUNNING | flags::IFF_LOWER_UP;
        } else {
            link.flags &= !(flags::IFF_UP | flags::IFF_RUNNING | flags::IFF_LOWER_UP);
        }
        if link.is_loopback() && up && link.addrs.is_empty() {
            link.addrs.push(IfAddr {
                addr: Ipv4Addr::LOCALHOST.into(),
                prefix_len: 8,
                scope: Scope::Host,
            });
            link.addrs.push(IfAddr {
                addr: Ipv6Addr::LOCALHOST.into(),
                prefix_len: 128,
                scope: Scope::Host,
            });
        }
        Ok(())
    }

    /// `__dev_change_flags`, what `SIOCSIFFLAGS` and `RTM_NEWLINK` do: the flags a request
    /// owns come from `flags`, the device's own state stays, and a change of `IFF_UP` brings
    /// the interface up or down ([`Interfaces::set_up`]). `IFF_PROMISC` and `IFF_ALLMULTI`
    /// follow `flags`, as no one else asks for them here.
    pub fn change_flags(&mut self, index: u32, flags: u32) -> NetResult<()> {
        let link = self
            .links
            .iter_mut()
            .find(|l| l.index == index)
            .ok_or(Errno::ENODEV)?;
        let old = link.flags;
        let kept = flags::IFF_UP | flags::IFF_VOLATILE;
        let listening = flags::IFF_PROMISC | flags::IFF_ALLMULTI;
        link.flags = (flags & (flags::IFF_SETTABLE | listening)) | (old & kept);
        if (old ^ flags) & flags::IFF_UP != 0 {
            self.set_up(index, flags & flags::IFF_UP != 0)?;
        }
        Ok(())
    }

    /// `ip addr add`; also installs the prefix route, as Linux does for a new address.
    pub fn add_addr(&mut self, index: u32, addr: IpAddr, prefix_len: u8) -> NetResult<()> {
        let max = if addr.is_ipv4() { 32 } else { 128 };
        if prefix_len > max {
            return Err(Errno::EINVAL);
        }
        let link = self
            .links
            .iter_mut()
            .find(|l| l.index == index)
            .ok_or(Errno::ENODEV)?;
        if link.addrs.iter().any(|a| a.addr == addr) {
            return Err(Errno::EEXIST);
        }
        let scope = match addr {
            IpAddr::V4(a) if a.is_loopback() => Scope::Host,
            IpAddr::V6(a) if a.is_loopback() => Scope::Host,
            IpAddr::V6(a) if a.is_unicast_link_local() => Scope::Link,
            _ => Scope::Universe,
        };
        link.addrs.push(IfAddr {
            addr,
            prefix_len,
            scope,
        });
        if !link.is_loopback() && prefix_len < max {
            self.routes.push(Route {
                dst: network_of(addr, prefix_len),
                prefix_len,
                gateway: None,
                oif: index,
                prefsrc: Some(addr),
                metric: 0,
            });
        }
        Ok(())
    }

    pub fn del_addr(&mut self, index: u32, addr: IpAddr) -> NetResult<()> {
        let link = self
            .links
            .iter_mut()
            .find(|l| l.index == index)
            .ok_or(Errno::ENODEV)?;
        let before = link.addrs.len();
        link.addrs.retain(|a| a.addr != addr);
        if link.addrs.len() == before {
            return Err(Errno::EADDRNOTAVAIL);
        }
        self.routes
            .retain(|r| r.prefsrc != Some(addr) || r.gateway.is_some());
        Ok(())
    }

    /// `ip route add`.
    pub fn add_route(&mut self, route: Route) -> NetResult<()> {
        if self.link(route.oif).is_none() {
            return Err(Errno::ENODEV);
        }
        if self.routes.iter().any(|r| {
            r.dst == route.dst && r.prefix_len == route.prefix_len && r.metric == route.metric
        }) {
            return Err(Errno::EEXIST);
        }
        self.routes.push(route);
        Ok(())
    }

    pub fn del_route(&mut self, dst: IpAddr, prefix_len: u8) -> NetResult<()> {
        let before = self.routes.len();
        self.routes
            .retain(|r| !(r.dst == dst && r.prefix_len == prefix_len));
        if self.routes.len() == before {
            return Err(Errno::ESRCH);
        }
        Ok(())
    }

    /// The interface that holds `ip`, if it is an address of an up interface. Every address
    /// in 127.0.0.0/8 is local while `lo` is up, as Linux's local route for the prefix makes it.
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

    /// `fib_lookup`: local addresses first, then the longest matching prefix (lowest metric on
    /// ties) over up interfaces. `None` is `ENETUNREACH`.
    pub fn route(&self, dst: IpAddr) -> Option<RouteDecision> {
        if let Some(oif) = self.local_link(dst) {
            return Some(RouteDecision {
                oif,
                src: dst,
                local: true,
            });
        }
        let route = self
            .routes
            .iter()
            .filter(|r| in_prefix(dst, r.dst, r.prefix_len))
            .filter(|r| self.link(r.oif).is_some_and(Link::is_up))
            .max_by(|a, b| {
                a.prefix_len
                    .cmp(&b.prefix_len)
                    .then(b.metric.cmp(&a.metric))
            })?;
        let src = route.prefsrc.or_else(|| {
            self.link(route.oif)?
                .addrs
                .iter()
                .find(|a| a.addr.is_ipv4() == dst.is_ipv4())
                .map(|a| a.addr)
        })?;
        Some(RouteDecision {
            oif: route.oif,
            src,
            local: false,
        })
    }
}

fn network_of(addr: IpAddr, len: u8) -> IpAddr {
    match addr {
        IpAddr::V4(a) => Ipv4Addr::from(u32::from(a) & prefix_mask_v4(len)).into(),
        IpAddr::V6(a) => Ipv6Addr::from(u128::from(a) & prefix_mask_v6(len)).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn fresh_namespace_has_lo_down_without_addresses() {
        let t = Interfaces::loopback_only(false);
        assert!(!t.links()[0].is_up());
        assert!(t.links()[0].addrs.is_empty());
        assert_eq!(t.route(ip("127.0.0.1")), None);
    }

    #[test]
    fn changing_flags_brings_lo_up_and_keeps_the_devices_own_state() {
        let mut t = Interfaces::loopback_only(false);
        t.change_flags(LOOPBACK_INDEX, flags::IFF_UP | flags::IFF_NOARP)
            .unwrap();
        let lo = &t.links()[0];
        let want = flags::IFF_UP
            | flags::IFF_LOOPBACK
            | flags::IFF_RUNNING
            | flags::IFF_LOWER_UP
            | flags::IFF_NOARP;
        assert_eq!(lo.flags, want);
        assert!(t.is_local(ip("127.0.0.1")));
        // Clearing IFF_LOOPBACK or IFF_RUNNING asks for nothing; IFF_UP left out takes it down.
        t.change_flags(LOOPBACK_INDEX, flags::IFF_PROMISC).unwrap();
        let lo = &t.links()[0];
        assert_eq!(lo.flags, flags::IFF_LOOPBACK | flags::IFF_PROMISC);
        assert_eq!(t.route(ip("127.0.0.1")), None);
        assert_eq!(t.change_flags(9, flags::IFF_UP), Err(Errno::ENODEV));
    }

    #[test]
    fn loopback_covers_127_slash_8() {
        let t = Interfaces::loopback_only(true);
        assert!(t.is_local(ip("127.0.0.1")));
        assert!(t.is_local(ip("127.1.2.3")));
        assert!(t.is_local(ip("::1")));
        assert!(!t.is_local(ip("10.0.0.1")));
        assert_eq!(t.route(ip("8.8.8.8")), None);
    }

    #[test]
    fn longest_prefix_and_source_selection() {
        let mut t = Interfaces::loopback_only(true);
        let eth = t.add_link("eth0", 1500, [2, 0, 0, 0, 0, 1]);
        t.set_up(eth, true).unwrap();
        t.add_addr(eth, ip("10.0.2.15"), 24).unwrap();
        t.add_route(Route {
            dst: ip("0.0.0.0"),
            prefix_len: 0,
            gateway: Some(ip("10.0.2.2")),
            oif: eth,
            prefsrc: None,
            metric: 100,
        })
        .unwrap();
        let d = t.route(ip("1.1.1.1")).unwrap();
        assert_eq!((d.oif, d.src, d.local), (eth, ip("10.0.2.15"), false));
        let d = t.route(ip("10.0.2.15")).unwrap();
        assert!(d.local);
        assert_eq!(
            t.links()[1].addrs[0].broadcast(),
            Some("10.0.2.255".parse().unwrap())
        );
        t.set_up(eth, false).unwrap();
        assert_eq!(t.route(ip("1.1.1.1")), None);
    }
}
