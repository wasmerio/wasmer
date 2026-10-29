//! A network namespace: port tables, interfaces and routes, the backend, and publishing.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use wasixv2_abi::Errno;

use wasixv2_wait::{PollTable, WaitQueue};

use crate::{
    Backend, BackendDatagram, BackendStream, Domain, EchoIgnore, IcmpEntry, IcmpLimit, InboundSink,
    Interface, NetResult, Network, NewLink, NewLinkKind, PortEntry, Publication, Room, Socket,
    SocketKind,
    addr::{canonical, canonical_ip},
    cmsg::SendCtl,
    icmp::{IPPROTO_ICMP, IPPROTO_ICMPV6, IcmpSocket, IcmpTables},
    iface::{
        AddMode, AddrMatch, IfAddr, Interfaces, LOOPBACK_INDEX, Link, LinkKind, Route,
        RouteDecision, RouteMatch, VethEnd, Via, valid_mac,
    },
    ports::{BindSpec, PortRange, PortRng, PortTable, Proto, SockId},
    sctp::{IPPROTO_SCTP, SctpPorts, SctpSocket},
    sync::MutexExt,
    tcp::{Offer, TcpSocket},
    udp::UdpSocket,
};

/// The lock every change of interfaces, addresses and routes takes first (`rtnl_lock`), so a
/// change that spans namespaces, such as moving an interface, happens as one. It comes before
/// any namespace's own lock.
static RTNL: Mutex<()> = Mutex::new(());

/// Proof that the caller holds the configuration lock (see [`rtnl`]).
pub struct Rtnl(#[allow(dead_code)] MutexGuard<'static, ()>);

/// Takes the configuration lock.
pub fn rtnl() -> Rtnl {
    Rtnl(RTNL.lock_or_recover())
}

/// An interface on its way between namespaces (see [`move_link`]).
pub struct MovedLink {
    link: Box<Link>,
}

impl MovedLink {
    pub fn name(&self) -> &str {
        &self.link.name
    }
}

/// `__dev_change_net_namespace`: moves interface `index` of `from` to `to`, where it arrives
/// down and without addresses, keeping its index if free there. `EEXIST` if `to` has an
/// interface of its name, `EINVAL` for one that stays in its namespace. Returns its index in
/// `to`.
pub fn move_link(from: &dyn Network, index: u32, to: &dyn Network) -> NetResult<u32> {
    let rtnl = rtnl();
    let link = from
        .interfaces()
        .into_iter()
        .find(|i| i.index == index)
        .ok_or(Errno::ENODEV)?;
    // Only what a request made can move (`netns_local` is checked first).
    if link.kind.is_none() {
        return Err(Errno::EINVAL);
    }
    if to.interfaces().iter().any(|i| i.name == link.name) {
        return Err(Errno::EEXIST);
    }
    let link = from.take_link(&rtnl, index)?;
    match to.adopt_link(&rtnl, link) {
        Ok(index) => {
            drop(rtnl);
            // The notification of the move names the new namespace by an id
            // (`peernet2id_alloc`), which it gets if it had none.
            match from.assign_peer_id(to, None) {
                Ok(_) | Err(Errno::EEXIST | Errno::EOPNOTSUPP) => {}
                Err(e) => tracing::warn!(?e, "no id for the namespace an interface moved to"),
            }
            Ok(index)
        }
        Err((e, link)) => {
            if let Err((back, _)) = from.adopt_link(&rtnl, link) {
                tracing::warn!(?back, "an interface that could not move is lost");
            }
            Err(e)
        }
    }
}

/// The host-facing interface of a namespace (`eth0`): the addresses the guest uses for
/// traffic that leaves through the backend.
#[derive(Clone, Debug)]
pub struct Uplink {
    pub name: String,
    pub mtu: u32,
    pub mac: [u8; 6],
    /// Address, prefix length and default gateway.
    pub v4: Option<(Ipv4Addr, u8, Option<Ipv4Addr>)>,
    pub v6: Option<(Ipv6Addr, u8, Option<Ipv6Addr>)>,
}

impl Default for Uplink {
    /// slirp's well-known layout: 10.0.2.15/24 behind gateway 10.0.2.2.
    fn default() -> Self {
        Self {
            name: "eth0".into(),
            mtu: 1500,
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            v4: Some((
                Ipv4Addr::new(10, 0, 2, 15),
                24,
                Some(Ipv4Addr::new(10, 0, 2, 2)),
            )),
            v6: None,
        }
    }
}

/// Configuration of a namespace's network.
#[derive(Clone, Debug)]
pub struct NetConfig {
    /// Whether `lo` starts up. True for a machine's first namespace; Linux starts a namespace
    /// made by `unshare(CLONE_NEWNET)` with `lo` down.
    pub loopback_up: bool,
    pub uplink: Option<Uplink>,
    /// `net.ipv4.ip_local_port_range`.
    pub port_range: PortRange,
    /// `net.ipv4.ip_unprivileged_port_start`.
    pub unprivileged_port_start: u16,
    /// Gateway alias policy: a guest address whose traffic goes to the host's loopback
    /// (`127.0.0.1` / `::1`) through the backend.
    pub host_loopback_alias: Option<IpAddr>,
    /// Auto-publishing (opt-in): mirror each guest listener, and each UDP socket the guest
    /// binds explicitly, on this host address, same port. Sockets bound only to a loopback
    /// address stay private.
    pub auto_publish: Option<IpAddr>,
    /// DNS policy: the name servers `resolv.conf` lists.
    pub nameservers: Vec<IpAddr>,
    /// The namespace's DNS server, as slirp's at 10.0.2.3: addresses on the uplink's subnets
    /// where a host answers DNS over UDP and TCP port 53 from the backend's resolver
    /// ([`crate::Backend::lookup`]) and echo requests, and refuses everything else.
    pub dns_server: Vec<IpAddr>,
    /// Seed for ephemeral port selection.
    pub seed: u64,
}

impl Default for NetConfig {
    /// Loopback only.
    fn default() -> Self {
        Self {
            loopback_up: true,
            uplink: None,
            port_range: PortRange::default(),
            unprivileged_port_start: 1024,
            host_loopback_alias: None,
            auto_publish: None,
            nameservers: Vec::new(),
            dns_server: Vec::new(),
            seed: 0x5eed,
        }
    }
}

impl NetConfig {
    /// A namespace behind a host backend: loopback plus the default uplink, with the gateway
    /// reaching the host's loopback and the DNS server at the uplink's [`Uplink::dns_server`]
    /// addresses, which `resolv.conf` names.
    pub fn host_default() -> Self {
        let uplink = Uplink::default();
        let gw = uplink.v4.and_then(|v| v.2).map(IpAddr::V4);
        let dns = uplink.dns_server();
        Self {
            uplink: Some(uplink),
            host_loopback_alias: gw,
            nameservers: dns.clone(),
            dns_server: dns,
            ..Self::default()
        }
    }
}

impl Uplink {
    /// Where slirp puts its DNS server: host 3 of each of the uplink's subnets (10.0.2.3 by
    /// default, `<prefix>::3` as slirp's `fec0::3`).
    pub fn dns_server(&self) -> Vec<IpAddr> {
        let v4 = self.v4.map(|(a, len, _)| {
            let mask = u32::MAX.checked_shl(32 - u32::from(len)).unwrap_or(0);
            IpAddr::V4(Ipv4Addr::from((u32::from(a) & mask) | 3))
        });
        let v6 = self.v6.map(|(a, len, _)| {
            let mask = u128::MAX.checked_shl(128 - u32::from(len)).unwrap_or(0);
            IpAddr::V6(Ipv6Addr::from((u128::from(a) & mask) | 3))
        });
        v4.into_iter().chain(v6).collect()
    }
}

/// `net.core.somaxconn`'s default, the host backlog of an explicit publication.
const PUBLISH_BACKLOG: u32 = 4096;

pub(crate) struct NetState {
    pub tcp: PortTable<Weak<TcpSocket>>,
    pub udp: PortTable<Weak<UdpSocket>>,
    pub udp_pubs: Vec<UdpPublication>,
    pub ifaces: Interfaces,
    pub rng: PortRng,
    /// The ids this namespace knows other namespaces by (`netns_ids`).
    pub nsids: Vec<(i32, Weak<Net>)>,
    pub icmp: IcmpTables,
    pub sctp: SctpPorts,
}

/// A published UDP port: the backend endpoint bound to the host port, and the guest socket
/// it serves, the first one bound to an address the publication reaches.
pub(crate) struct UdpPublication {
    id: u64,
    /// Canonical.
    guest: SocketAddr,
    endpoint: Arc<dyn BackendDatagram>,
    attached: Option<(SockId, Weak<UdpSocket>)>,
}

/// Where a destination goes.
pub(crate) enum Path {
    /// To a socket of this namespace (`net` is `None`), or across a veth of the peer's.
    Local {
        src: IpAddr,
        net: Option<Arc<Net>>,
    },
    Backend {
        src: IpAddr,
        host_dst: SocketAddr,
    },
    /// To the namespace's DNS server ([`NetConfig::dns_server`]), out the uplink.
    Dns {
        src: IpAddr,
    },
    /// Out an interface where nothing answers: a dummy, a veth without carrier, or one whose
    /// peer does not hold the destination (there is no forwarding). `unresolved` when
    /// neighbour resolution of the next hop fails there.
    Nowhere {
        src: IpAddr,
        unresolved: bool,
    },
}

impl Path {
    pub(crate) fn src(&self) -> IpAddr {
        match self {
            Path::Local { src, .. }
            | Path::Backend { src, .. }
            | Path::Dns { src }
            | Path::Nowhere { src, .. } => *src,
        }
    }
}

/// A route decision made under the namespace's lock, before the peer of a veth was asked.
enum Hop {
    Done(Path),
    /// Out the veth end `oif`, to the next hop `hop` (the route's gateway, else the
    /// destination).
    Veth {
        src: IpAddr,
        end: VethEnd,
        oif: u32,
        hop: IpAddr,
    },
}

/// Where an IP packet to an address goes, with the interfaces it arrives on and an answer
/// comes back in on.
pub(crate) enum IpPath {
    /// To the namespace `net` that holds the address (this one, or the one across a veth),
    /// arriving on `iif` there; an answer comes back in on `back_iif` here.
    Local {
        src: IpAddr,
        net: Arc<Net>,
        iif: u32,
        back_iif: u32,
    },
    Backend {
        src: IpAddr,
        host_dst: SocketAddr,
    },
    /// To the namespace's DNS server.
    Dns {
        src: IpAddr,
    },
    Nowhere {
        src: IpAddr,
        unresolved: bool,
    },
}

impl IpPath {
    pub(crate) fn src(&self) -> IpAddr {
        match self {
            IpPath::Local { src, .. }
            | IpPath::Backend { src, .. }
            | IpPath::Dns { src }
            | IpPath::Nowhere { src, .. } => *src,
        }
    }
}

/// One network namespace. Implements [`Network`].
pub struct Net {
    state: Mutex<NetState>,
    pub(crate) backend: Option<Arc<dyn Backend>>,
    pub(crate) config: NetConfig,
    next_id: AtomicU64,
    me: Weak<Net>,
}

impl Net {
    pub fn new(config: NetConfig, backend: Option<Arc<dyn Backend>>) -> Arc<Self> {
        let mut ifaces = Interfaces::loopback_only(config.loopback_up);
        if let Some(up) = &config.uplink {
            let idx = ifaces
                .add_link(&up.name, LinkKind::Uplink, up.mtu, up.mac)
                .expect("a valid uplink name");
            ifaces.set_up(idx, true).expect("link exists");
            let v4 = up
                .v4
                .map(|(a, len, gw)| (a.into(), len, gw.map(IpAddr::from)));
            let v6 = up
                .v6
                .map(|(a, len, gw)| (a.into(), len, gw.map(IpAddr::from)));
            for (a, len, gw) in [v4, v6].into_iter().flatten() {
                // As DHCP sets it up: with the subnet's broadcast address.
                let mut addr = IfAddr::new(a, len);
                addr.broadcast = addr.subnet_broadcast();
                ifaces
                    .add_addr(idx, addr, AddMode::ADD)
                    .expect("valid uplink address");
                if let Some(gw) = gw {
                    let any = match a {
                        IpAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
                        IpAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
                    };
                    ifaces
                        .add_route(Route::unicast(any, 0, Some(gw), idx), AddMode::ADD)
                        .expect("a gateway on the uplink's subnet");
                }
            }
        }
        let seed = config.seed;
        Arc::new_cyclic(|me| Self {
            state: Mutex::new(NetState {
                tcp: PortTable::new(Proto::Tcp),
                udp: PortTable::new(Proto::Udp),
                udp_pubs: Vec::new(),
                ifaces,
                rng: PortRng::new(seed),
                nsids: Vec::new(),
                icmp: IcmpTables::default(),
                sctp: SctpPorts::default(),
            }),
            backend,
            config,
            next_id: AtomicU64::new(1),
            me: me.clone(),
        })
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, NetState> {
        self.state.lock_or_recover()
    }

    /// The namespace's state for a configuration change, which `_rtnl` serialises, without
    /// the veth ends whose peer's namespace is gone.
    fn configure(&self, _rtnl: &Rtnl) -> MutexGuard<'_, NetState> {
        let mut st = self.state();
        st.prune();
        st
    }

    pub(crate) fn arc(&self) -> Arc<Net> {
        self.me.upgrade().expect("network alive while used")
    }

    pub(crate) fn next_id(&self) -> SockId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Changes the interface and route tables (`ip link`, `ip addr`, `ip route`).
    pub fn update_interfaces<R>(&self, f: impl FnOnce(&mut Interfaces) -> R) -> R {
        let rtnl = rtnl();
        f(&mut self.configure(&rtnl).ifaces)
    }

    /// `fib_lookup` for `dst`.
    pub fn route(&self, dst: IpAddr) -> NetResult<RouteDecision> {
        self.state().ifaces.route(dst)
    }

    /// The id this namespace knows `other` by (`peernet2id`), given one (`peernet2id_alloc`)
    /// if `alloc`; `None` for itself.
    fn nsid(&self, st: &mut NetState, other: &Weak<Net>, alloc: bool) -> Option<i32> {
        if Weak::ptr_eq(other, &self.me) {
            return None;
        }
        st.nsids.retain(|(_, n)| n.strong_count() > 0);
        if let Some((id, _)) = st.nsids.iter().find(|(_, n)| Weak::ptr_eq(n, other)) {
            return Some(*id);
        }
        if !alloc {
            return None;
        }
        let id = Self::free_nsid(st);
        st.nsids.push((id, other.clone()));
        Some(id)
    }

    /// `alloc_netid` without a wish: the lowest id not in use.
    fn free_nsid(st: &NetState) -> i32 {
        (0..)
            .find(|i| st.nsids.iter().all(|(id, _)| id != i))
            .expect("ids left")
    }

    /// `eth_random_addr`: a random, locally administered unicast address.
    fn random_mac(st: &mut NetState) -> [u8; 6] {
        let mut mac = [0u8; 6];
        for b in &mut mac {
            *b = st.rng.below(256) as u8;
        }
        mac[0] = (mac[0] & 0xfe) | 0x02;
        mac
    }

    /// DNS policy: the guest's `/etc/resolv.conf`.
    pub fn resolv_conf(&self) -> String {
        self.config
            .nameservers
            .iter()
            .map(|ns| format!("nameserver {ns}\n"))
            .collect()
    }

    /// Explicit publishing: connections the backend accepts on `host` go to the guest
    /// listener for `guest`. The host listener's backlog is `somaxconn`'s default, the most a
    /// guest listener could ask for; connections wait there while the guest's accept queue
    /// is full.
    pub fn publish(
        &self,
        host: SocketAddr,
        guest: SocketAddr,
    ) -> std::io::Result<Box<dyn Publication>> {
        let backend = self.backend.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Unsupported, "network has no backend")
        })?;
        backend.publish(host, PUBLISH_BACKLOG, self.inbound_sink(guest))
    }

    /// Explicit publishing of a UDP port: datagrams the backend receives on `host` go to the
    /// guest socket bound to `guest` (for a wildcard, to the address the guest sees them
    /// arrive on, as for [`Net::publish`]), and that socket's datagrams to outside addresses
    /// leave from `host`, so replies come from the published port. Of several sockets sharing
    /// the port (`SO_REUSEADDR`, `SO_REUSEPORT`) the first bound is served, the next once it
    /// goes.
    pub fn publish_udp(
        &self,
        host: SocketAddr,
        guest: SocketAddr,
    ) -> std::io::Result<Box<dyn Publication>> {
        let backend = self.backend.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Unsupported, "network has no backend")
        })?;
        let (endpoint, host) = backend.publish_datagram(host)?;
        let id = self.next_id();
        self.state().udp_pubs.push(UdpPublication {
            id,
            guest: canonical(guest),
            endpoint,
            attached: None,
        });
        self.offer_udp_publication(id);
        Ok(Box::new(UdpPublished {
            net: self.me.clone(),
            id,
            host,
        }))
    }

    /// Hands publication `id`, if unattached, to a socket already bound where it reaches.
    /// Called without socket locks held.
    pub(crate) fn offer_udp_publication(&self, id: u64) {
        let socks: Vec<_> = {
            let st = self.state();
            st.udp.entries().map(|(_, w)| w.clone()).collect()
        };
        for sock in socks.iter().filter_map(Weak::upgrade) {
            if sock.adopt_publication(id) {
                return;
            }
        }
    }

    /// Called with the socket's lock held, for a socket bound to `local`: attaches it to a
    /// publication that reaches it (`id`, or any) and no socket holds.
    pub(crate) fn claim_udp_publication(
        &self,
        local: SocketAddr,
        v6only: bool,
        sock: (SockId, Weak<UdpSocket>),
        id: Option<u64>,
    ) -> Option<(u64, Arc<dyn BackendDatagram>)> {
        let mut st = self.state();
        let i = st.udp_pubs.iter().position(|p| {
            p.attached.is_none()
                && id.is_none_or(|id| id == p.id)
                && serves(local, v6only, self.inbound_destination(&st, p.guest, None))
        })?;
        let p = &mut st.udp_pubs[i];
        p.attached = Some(sock);
        Some((p.id, p.endpoint.clone()))
    }

    /// Socket `sock` no longer serves publication `id` (it unbound or went).
    pub(crate) fn release_udp_publication(&self, id: u64, sock: SockId) {
        let mut st = self.state();
        if let Some(p) = st.udp_pubs.iter_mut().find(|p| p.id == id)
            && p.attached.as_ref().is_some_and(|(s, _)| *s == sock)
        {
            p.attached = None;
        }
    }

    /// The guest address traffic through a publication for `guest` arrives at: `guest`, or
    /// for a wildcard the uplink address the guest reaches `peer` (any peer if unknown) from,
    /// else loopback.
    fn inbound_destination(
        &self,
        st: &NetState,
        guest: SocketAddr,
        peer: Option<SocketAddr>,
    ) -> SocketAddr {
        let mut dst = guest;
        if dst.ip().is_unspecified() {
            let any: IpAddr = match guest {
                SocketAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
                SocketAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
            };
            let ip = st
                .ifaces
                .route(peer.map_or(any, |p| p.ip()))
                .ok()
                .filter(|d| !d.local)
                .map(|d| d.src)
                .unwrap_or(if dst.is_ipv4() {
                    Ipv4Addr::LOCALHOST.into()
                } else {
                    Ipv6Addr::LOCALHOST.into()
                });
            dst.set_ip(ip);
        }
        dst
    }

    pub(crate) fn inbound_sink(&self, guest: SocketAddr) -> Arc<dyn InboundSink> {
        Arc::new(GuestListenerSink {
            net: self.me.clone(),
            guest: canonical(guest),
        })
    }

    /// Where traffic to `dst` (canonical) goes, given the socket's bound address.
    pub(crate) fn path(&self, dst: SocketAddr, bound: Option<IpAddr>) -> NetResult<Path> {
        let hop = self.hop(&self.state(), dst, bound)?;
        Ok(match hop {
            Hop::Done(path) => path,
            Hop::Veth { src, end, hop, .. } => across(src, &end, dst, hop),
        })
    }

    /// The route to `dst` as far as this namespace decides it.
    fn hop(&self, st: &NetState, dst: SocketAddr, bound: Option<IpAddr>) -> NetResult<Hop> {
        let decision = st.ifaces.route(dst.ip())?;
        let src = bound
            .filter(|ip| !ip.is_unspecified())
            .unwrap_or(decision.src);
        let hop = next_hop(&decision, dst.ip());
        Ok(match decision.via {
            Via::Local => Hop::Done(Path::Local { src, net: None }),
            Via::Uplink if self.is_dns_server(dst.ip()) => Hop::Done(Path::Dns { src }),
            Via::Uplink if self.backend.is_some() => Hop::Done(Path::Backend {
                src,
                host_dst: self.host_view(dst),
            }),
            Via::Uplink => return Err(Errno::ENETUNREACH),
            Via::Veth(end) => Hop::Veth {
                src,
                end,
                oif: decision.oif,
                hop,
            },
            Via::Nowhere { unresolved } => Hop::Done(Path::Nowhere { src, unresolved }),
        })
    }

    /// The MTU of the interface `index` (`dst->dev->mtu`; no path MTU is ever learned).
    pub(crate) fn link_mtu(&self, index: u32) -> Option<u32> {
        self.state().ifaces.link(index).map(|l| l.mtu)
    }

    /// The MTU of the interface a packet to `dst` leaves by, `None` without a route.
    pub(crate) fn mtu_to(&self, dst: IpAddr) -> Option<u32> {
        let st = self.state();
        let oif = st.ifaces.route(dst).ok()?.oif;
        st.ifaces.link(oif).map(|l| l.mtu)
    }

    /// The uplink's interface index, 0 if the namespace has none.
    pub(crate) fn uplink_index(&self) -> u32 {
        let st = self.state();
        st.ifaces
            .links()
            .iter()
            .find(|l| matches!(l.kind, LinkKind::Uplink))
            .map_or(0, |l| l.index)
    }

    /// Gateway alias: the alias address means the host's loopback.
    pub(crate) fn host_view(&self, dst: SocketAddr) -> SocketAddr {
        match self.config.host_loopback_alias {
            Some(alias) if alias == dst.ip() => {
                let lo: IpAddr = if alias.is_ipv4() {
                    Ipv4Addr::LOCALHOST.into()
                } else {
                    Ipv6Addr::LOCALHOST.into()
                };
                SocketAddr::new(lo, dst.port())
            }
            _ => dst,
        }
    }

    /// The reverse of [`Net::host_view`] for addresses the backend reports.
    pub(crate) fn guest_view(&self, src: SocketAddr) -> SocketAddr {
        let src = canonical(src);
        match self.config.host_loopback_alias {
            Some(alias) if src.ip().is_loopback() && alias.is_ipv4() == src.is_ipv4() => {
                SocketAddr::new(alias, src.port())
            }
            _ => src,
        }
    }

    /// Validates a bind address (`inet_addr_valid_or_nonlocal`) and the low-port check.
    pub(crate) fn check_bind(
        &self,
        addr: SocketAddr,
        privileged: bool,
        freebind: bool,
    ) -> NetResult<()> {
        let ip = addr.ip();
        let local = ip.is_unspecified()
            || ip.is_multicast()
            || ip == IpAddr::V4(Ipv4Addr::BROADCAST)
            || self.state().ifaces.is_local(ip);
        if !local && !freebind {
            return Err(Errno::EADDRNOTAVAIL);
        }
        if addr.port() != 0 && addr.port() < self.config.unprivileged_port_start && !privileged {
            return Err(Errno::EACCES);
        }
        Ok(())
    }

    pub(crate) fn tcp_bind(
        &self,
        id: SockId,
        spec: BindSpec,
        port: u16,
        sock: Weak<TcpSocket>,
    ) -> NetResult<u16> {
        let mut st = self.state();
        let st = &mut *st;
        st.tcp
            .bind(id, spec, port, self.config.port_range, &mut st.rng, sock)
    }

    pub(crate) fn tcp_bind_connect(
        &self,
        id: SockId,
        spec: BindSpec,
        remote: SocketAddr,
        sock: Weak<TcpSocket>,
    ) -> NetResult<u16> {
        let mut st = self.state();
        let st = &mut *st;
        st.tcp
            .bind_connect(id, spec, remote, self.config.port_range, &mut st.rng, sock)
    }

    pub(crate) fn udp_bind(
        &self,
        id: SockId,
        spec: BindSpec,
        port: u16,
        sock: Weak<UdpSocket>,
    ) -> NetResult<u16> {
        let mut st = self.state();
        let st = &mut *st;
        st.udp
            .bind(id, spec, port, self.config.port_range, &mut st.rng, sock)
    }

    /// The best listener for a connection to `dst` from `src`; a `SO_REUSEPORT` group is
    /// spread by the source port.
    pub(crate) fn find_listener(&self, dst: SocketAddr, src: SocketAddr) -> Option<Arc<TcpSocket>> {
        self.listener_group(dst, src).into_iter().next()
    }

    /// Every listener a connection to `dst` from `src` may go to, the one `find_listener`
    /// picks first and the rest of its `SO_REUSEPORT` group after it.
    fn listener_group(&self, dst: SocketAddr, src: SocketAddr) -> Vec<Arc<TcpSocket>> {
        let candidates = self
            .state()
            .tcp
            .lookup(dst, Some(src), |_, listening| listening);
        let mut live: Vec<_> = candidates.iter().filter_map(Weak::upgrade).collect();
        if !live.is_empty() {
            let i = src.port() as usize % live.len();
            live.rotate_left(i);
        }
        live
    }

    /// Where a UDP datagram from the bound `local` to `dst` goes, decided under one lock: the
    /// route (`ip_route_output_flow`, with the source and interface the control messages
    /// chose) and inside the namespace the receiving socket.
    pub(crate) fn udp_route(
        &self,
        local: SocketAddr,
        dst: SocketAddr,
        ctl: &SendCtl,
    ) -> NetResult<UdpRoute> {
        let (hop, to) = {
            let st = self.state();
            // `ip_route_output_key_hash_rcu`: an unknown interface is ENODEV, a source address
            // that is not the machine's EINVAL.
            if ctl.oif != 0 && st.ifaces.link(ctl.oif).is_none() {
                return Err(Errno::ENODEV);
            }
            let src = ctl.src.map(canonical_ip);
            if let Some(src) = src
                && !st.ifaces.is_local(src)
            {
                return Err(Errno::EINVAL);
            }
            let hop = self.hop(&st, dst, src.or(Some(local.ip())))?;
            let to = match &hop {
                Hop::Done(Path::Local { src, .. }) => udp_receiver(&st, *src, local, dst),
                _ => None,
            };
            (hop, to)
        };
        Ok(match hop {
            Hop::Done(Path::Local { src, .. }) => UdpRoute::Local {
                from: SocketAddr::new(src, local.port()),
                to: to.and_then(|w| w.upgrade()),
                at: self.arc(),
                iif: LOOPBACK_INDEX,
                back_iif: LOOPBACK_INDEX,
            },
            Hop::Done(Path::Backend { host_dst, .. }) => UdpRoute::Backend { host_dst },
            Hop::Done(Path::Dns { src }) => UdpRoute::Dns {
                from: SocketAddr::new(src, local.port()),
            },
            Hop::Done(Path::Nowhere { src, unresolved }) => UdpRoute::Nowhere {
                from: SocketAddr::new(src, local.port()),
                unresolved,
            },
            Hop::Veth { src, end, oif, hop } => match across(src, &end, dst, hop) {
                Path::Local {
                    src,
                    net: Some(peer),
                } => {
                    let to = udp_receiver(&peer.state(), src, local, dst);
                    UdpRoute::Local {
                        from: SocketAddr::new(src, local.port()),
                        to: to.and_then(|w| w.upgrade()),
                        at: peer,
                        iif: end.peer().1,
                        back_iif: oif,
                    }
                }
                path => UdpRoute::Nowhere {
                    from: SocketAddr::new(src, local.port()),
                    unresolved: matches!(
                        path,
                        Path::Nowhere {
                            unresolved: true,
                            ..
                        }
                    ),
                },
            },
        })
    }

    /// Where a packet to `dst` from a socket bound to `bound` goes ([`IpPath`]).
    pub(crate) fn ip_path(&self, dst: IpAddr, bound: Option<IpAddr>) -> NetResult<IpPath> {
        let st = self.state();
        let decision = st.ifaces.route(dst)?;
        let src = bound
            .filter(|ip| !ip.is_unspecified())
            .unwrap_or(decision.src);
        let hop = next_hop(&decision, dst);
        Ok(match decision.via {
            Via::Local => IpPath::Local {
                src,
                net: self.arc(),
                iif: LOOPBACK_INDEX,
                back_iif: LOOPBACK_INDEX,
            },
            Via::Uplink if self.is_dns_server(dst) => IpPath::Dns { src },
            Via::Uplink if self.backend.is_some() => IpPath::Backend {
                src,
                host_dst: self.host_view(SocketAddr::new(dst, 0)),
            },
            Via::Uplink => return Err(Errno::ENETUNREACH),
            Via::Veth(end) => {
                drop(st);
                let (peer, iif) = end.peer();
                match peer.upgrade() {
                    Some(net) if net.state().ifaces.is_local(dst) => IpPath::Local {
                        src,
                        net,
                        iif,
                        back_iif: decision.oif,
                    },
                    Some(net) => IpPath::Nowhere {
                        src,
                        unresolved: !net.state().ifaces.is_local(hop),
                    },
                    None => IpPath::Nowhere {
                        src,
                        unresolved: true,
                    },
                }
            }
            Via::Nowhere { unresolved } => IpPath::Nowhere { src, unresolved },
        })
    }
}

/// The socket of the namespace `st` that receives a datagram to `dst` from `src` and the port
/// of `local`, looked up without upgrading: a last reference must not drop under the lock.
pub(crate) fn udp_receiver(
    st: &NetState,
    src: IpAddr,
    local: SocketAddr,
    dst: SocketAddr,
) -> Option<Weak<UdpSocket>> {
    let from = SocketAddr::new(src, local.port());
    let live = |w: &Weak<UdpSocket>, _| w.strong_count() > 0;
    st.udp
        .lookup_one(dst, Some(from), live, from.port() as usize)
}

/// The neighbour a packet to `dst` goes to: the route's gateway, else `dst` itself.
fn next_hop(decision: &RouteDecision, dst: IpAddr) -> IpAddr {
    decision
        .route
        .as_ref()
        .and_then(|r| r.gateway)
        .unwrap_or(dst)
}

/// Across a veth: the peer's namespace if it holds `dst`, else nowhere, as Linux without
/// forwarding drops what it receives for an address it does not have (`ip_error` sends no
/// ICMP while forwarding is off). Neighbour resolution fails when the peer does not hold
/// the next hop `hop` either.
fn across(src: IpAddr, end: &VethEnd, dst: SocketAddr, hop: IpAddr) -> Path {
    let (net, _) = end.peer();
    match net.upgrade() {
        Some(net) if net.state().ifaces.is_local(dst.ip()) => Path::Local {
            src,
            net: Some(net),
        },
        Some(net) => Path::Nowhere {
            src,
            unresolved: !net.state().ifaces.is_local(hop),
        },
        None => Path::Nowhere {
            src,
            unresolved: true,
        },
    }
}

impl NetState {
    /// Deletes the veth ends whose peer's namespace is gone: Linux deletes both ends of a
    /// pair as either namespace exits.
    fn prune(&mut self) {
        let orphans: Vec<u32> = self
            .ifaces
            .links()
            .iter()
            .filter(|l| matches!(&l.kind, LinkKind::Veth(end) if end.orphaned()))
            .map(|l| l.index)
            .collect();
        for index in orphans {
            let _ = self.ifaces.remove(index);
        }
    }
}

/// Whether a UDP socket bound to `local` receives datagrams for `dst` (`compute_score`'s
/// address match: the same address, or a wildcard of a family that takes it).
pub(crate) fn serves(local: SocketAddr, v6only: bool, dst: SocketAddr) -> bool {
    local.port() == dst.port()
        && match local.ip() {
            ip if ip == dst.ip() => true,
            IpAddr::V4(v4) => v4.is_unspecified() && dst.is_ipv4(),
            IpAddr::V6(v6) => v6.is_unspecified() && (dst.is_ipv6() || !v6only),
        }
}

/// The handle [`Net::publish_udp`] returns: dropping it ends the publication, detaching the
/// socket it served.
struct UdpPublished {
    net: Weak<Net>,
    id: u64,
    /// The backend's handle, for the bound host address.
    host: Box<dyn Publication>,
}

impl Publication for UdpPublished {
    fn host_addr(&self) -> SocketAddr {
        self.host.host_addr()
    }
}

impl Drop for UdpPublished {
    fn drop(&mut self) {
        let Some(net) = self.net.upgrade() else {
            return;
        };
        let removed = {
            let mut st = net.state();
            let i = st.udp_pubs.iter().position(|p| p.id == self.id);
            i.map(|i| st.udp_pubs.remove(i))
        };
        // The socket and the endpoint, possibly their last references, go outside the lock.
        let Some(p) = removed else {
            return;
        };
        if let Some(sock) = p.attached.and_then(|(_, w)| w.upgrade()) {
            sock.lose_publication(p.id);
        }
    }
}

/// Where one UDP datagram goes (see [`Net::udp_route`]).
pub(crate) enum UdpRoute {
    /// Inside the namespace, from `from`, to the socket bound for it if any.
    Local {
        from: SocketAddr,
        to: Option<Arc<UdpSocket>>,
        /// The namespace holding the destination, which answers a datagram nobody takes
        /// with a port unreachable that comes back in on `back_iif`.
        at: Arc<Net>,
        /// The interface it arrives on there.
        iif: u32,
        back_iif: u32,
    },
    /// Out through the backend to `host_dst`.
    Backend { host_dst: SocketAddr },
    /// To the namespace's DNS server, from `from`.
    Dns { from: SocketAddr },
    /// Out an interface where nothing answers: the datagram is lost, from `from` (the
    /// route's source and the socket's port); `unresolved` when neighbour resolution fails.
    Nowhere { from: SocketAddr, unresolved: bool },
}

impl Network for Net {
    fn socket(
        &self,
        domain: Domain,
        kind: SocketKind,
        protocol: i32,
        uid: u32,
    ) -> Result<Arc<dyn Socket>, Errno> {
        // `inet_create`: 0 means the default; IPPROTO_TCP=6, IPPROTO_UDP=17, ICMP's datagram
        // (ping) sockets, and raw sockets of any protocol (see `raw.rs`).
        let icmp = match domain {
            Domain::Inet => IPPROTO_ICMP,
            Domain::Inet6 => IPPROTO_ICMPV6,
        };
        match (kind, protocol) {
            (SocketKind::Stream, 0 | 6) => Ok(TcpSocket::new(self.arc(), domain, uid)),
            // `inetsw` has SCTP for both of its styles, and is the only `SOCK_SEQPACKET`
            // protocol, which protocol 0 picks.
            (SocketKind::Stream, IPPROTO_SCTP) | (SocketKind::SeqPacket, 0 | IPPROTO_SCTP) => {
                Ok(SctpSocket::new(self.arc(), domain, kind, uid))
            }
            (SocketKind::Datagram, 0 | 17) => Ok(UdpSocket::new(self.arc(), domain, uid)),
            (SocketKind::Datagram, p) if p == icmp => {
                Ok(IcmpSocket::new(self.arc(), domain, false, uid))
            }
            (SocketKind::Raw, p) if p == icmp => Ok(IcmpSocket::new(self.arc(), domain, true, uid)),
            (SocketKind::Raw, 0) => Err(Errno::EPROTONOSUPPORT),
            (SocketKind::Raw, p) if !(0..crate::raw::IPPROTO_MAX).contains(&p) => {
                Err(Errno::EINVAL)
            }
            // `inet_num` is what a packet's protocol is matched against: one past 255 never is.
            (SocketKind::Raw, p) => Ok(IcmpSocket::raw(self.arc(), domain, p as u8, uid)),
            _ => Err(Errno::EPROTONOSUPPORT),
        }
    }

    fn vsock_cid(&self) -> Option<u32> {
        self.backend.as_ref()?.vsock_cid()
    }

    fn vsock_connect(
        &self,
        cid: u32,
        port: u32,
        wq: WaitQueue,
    ) -> Result<Arc<dyn BackendStream>, Errno> {
        match &self.backend {
            Some(backend) => backend.vsock_connect(cid, port, wq),
            None => Err(Errno::ENODEV),
        }
    }

    fn ports(&self) -> Vec<PortEntry> {
        // Sockets lock themselves before the namespace, so the entries are read only after
        // the namespace lock is gone; the strong references, possibly the last ones, are also
        // dropped outside it, since a socket's drop unbinds under that lock.
        let (tcp, udp): (Vec<_>, Vec<_>) = {
            let st = self.state();
            (
                st.tcp.entries().map(|(_, w)| w.clone()).collect(),
                st.udp.entries().map(|(_, w)| w.clone()).collect(),
            )
        };
        let tcp = tcp
            .iter()
            .filter_map(Weak::upgrade)
            .filter_map(|s| s.port_entry());
        let udp = udp
            .iter()
            .filter_map(Weak::upgrade)
            .filter_map(|s| s.port_entry());
        tcp.chain(udp).collect()
    }

    fn icmp_sockets(&self) -> Vec<IcmpEntry> {
        self.icmp_entries()
    }

    fn vsock_listen(
        &self,
        port: u32,
        backlog: u32,
        sink: Arc<dyn crate::VsockInboundSink>,
    ) -> Result<Box<dyn crate::VsockPublication>, Errno> {
        self.backend
            .as_ref()
            .ok_or(Errno::ENODEV)?
            .vsock_listen(port, backlog, sink)
    }

    fn sctp_endpoints(&self) -> Vec<crate::SctpEndpoint> {
        crate::sctp::endpoints(self)
    }

    fn sctp_assocs(&self) -> Vec<crate::SctpAssocEntry> {
        crate::sctp::assocs(self)
    }

    fn sctp_snmp(&self) -> Vec<(&'static str, u64)> {
        crate::sctp::snmp(self)
    }

    fn interfaces(&self) -> Vec<Interface> {
        let mut st = self.state();
        st.prune();
        let links = st.ifaces.links().to_vec();
        links
            .iter()
            .map(|l| Interface {
                index: l.index,
                name: l.name.clone(),
                flags: l.flags(),
                mtu: l.mtu,
                mac: l.mac,
                kind: l.kind.name(),
                peer: match &l.kind {
                    LinkKind::Veth(end) => {
                        let (net, index) = end.peer();
                        Some((index, self.nsid(&mut st, &net, true)))
                    }
                    _ => None,
                },
                addresses: l.addrs.clone(),
                ipv6_disabled: l.ipv6_disabled,
                groups: l.groups(),
                sources: l.group_sources(),
            })
            .collect()
    }

    fn set_icmp_echo_ignore(&self, which: EchoIgnore, ignore: bool) -> Result<(), Errno> {
        self.set_echo_ignore(which, ignore);
        Ok(())
    }

    fn set_icmp_limit(&self, limit: IcmpLimit) -> Result<(), Errno> {
        self.set_limit(limit);
        Ok(())
    }

    fn set_ipv6_disabled(&self, index: u32, disabled: bool) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl)
            .ifaces
            .set_ipv6_disabled(index, disabled)
    }

    fn change_flags(&self, index: u32, flags: u32) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.change_flags(index, flags)
    }

    fn net(&self) -> Option<Weak<Net>> {
        Some(self.me.clone())
    }

    fn peer_id(&self, other: &dyn Network) -> Option<i32> {
        let other = other.net()?;
        self.nsid(&mut self.state(), &other, false)
    }

    /// `rtnl_net_newid`.
    fn assign_peer_id(&self, other: &dyn Network, id: Option<i32>) -> Result<i32, Errno> {
        let other = other.net().ok_or(Errno::EOPNOTSUPP)?;
        let rtnl = rtnl();
        let mut st = self.configure(&rtnl);
        if self.nsid(&mut st, &other, false).is_some() || Weak::ptr_eq(&other, &self.me) {
            return Err(Errno::EEXIST);
        }
        let id = match id {
            Some(id) if id < 0 => return Err(Errno::EINVAL),
            Some(id) if st.nsids.iter().any(|(used, _)| *used == id) => return Err(Errno::EEXIST),
            Some(id) => id,
            None => Self::free_nsid(&st),
        };
        st.nsids.push((id, other));
        Ok(id)
    }

    fn peer_ids(&self) -> Vec<i32> {
        let mut st = self.state();
        st.nsids.retain(|(_, n)| n.strong_count() > 0);
        let mut ids: Vec<i32> = st.nsids.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids
    }

    fn routes(&self) -> Vec<Route> {
        self.state().ifaces.routes().to_vec()
    }

    fn route_get(&self, dst: IpAddr) -> Result<RouteDecision, Errno> {
        self.route(dst)
    }

    /// `veth_newlink` makes the peer first, so it has the lower index; `dummy_setup`'s and
    /// `veth_setup`'s defaults otherwise, with random addresses (`eth_hw_addr_random`).
    fn new_link(&self, spec: NewLink) -> Result<(u32, Option<u32>), Errno> {
        let rtnl = rtnl();
        let mut st = self.configure(&rtnl);
        let st = &mut *st;
        let mac = |given: Option<[u8; 6]>, st: &mut NetState| match given {
            Some(mac) if !valid_mac(mac) => Err(Errno::EADDRNOTAVAIL),
            Some(mac) => Ok(mac),
            None => Ok(Self::random_mac(st)),
        };
        let link = |index, name, kind: LinkKind, mtu: Option<u32>, mac| -> NetResult<Link> {
            let mtu = mtu.unwrap_or(1500);
            kind.check_mtu(mtu)?;
            Ok(Link {
                index,
                name,
                flags: kind.initial_flags(),
                mtu,
                mac,
                kind,
                addrs: Vec::new(),
                ipv6_disabled: false,
                joined: Vec::new(),
                filters: Vec::new(),
            })
        };
        match spec.kind {
            NewLinkKind::Dummy => {
                let name = st
                    .ifaces
                    .check_name(spec.name.as_deref().unwrap_or("dummy%d"))?;
                let mac = mac(spec.mac, st)?;
                LinkKind::Dummy.check_mtu(spec.mtu.unwrap_or(1500))?;
                let index = st.ifaces.reserve_index(None);
                st.ifaces
                    .insert(link(index, name, LinkKind::Dummy, spec.mtu, mac)?);
                Ok((index, None))
            }
            NewLinkKind::Veth {
                peer_name,
                peer_mtu,
                peer_mac,
            } => {
                // The peer is registered first, so a pattern in the device's name skips it.
                let peer_name = st
                    .ifaces
                    .check_name(peer_name.as_deref().unwrap_or("veth%d"))?;
                let peer_mac = mac(peer_mac, st)?;
                let mac = mac(spec.mac, st)?;
                let peer = st.ifaces.reserve_index(None);
                let index = st.ifaces.reserve_index(None);
                let (end, peer_end) = VethEnd::pair(self.me.clone(), index, peer);
                let peer_link = link(
                    peer,
                    peer_name,
                    LinkKind::Veth(peer_end),
                    peer_mtu,
                    peer_mac,
                )?;
                st.ifaces.insert(peer_link);
                let dev = st
                    .ifaces
                    .check_name(spec.name.as_deref().unwrap_or("veth%d"))
                    .and_then(|name| link(index, name, LinkKind::Veth(end), spec.mtu, mac));
                match dev {
                    Ok(dev) => st.ifaces.insert(dev),
                    Err(e) => {
                        let _ = st.ifaces.remove(peer);
                        return Err(e);
                    }
                }
                Ok((index, Some(peer)))
            }
        }
    }

    fn del_link(&self, index: u32) -> Result<(), Errno> {
        let rtnl = rtnl();
        let mut st = self.configure(&rtnl);
        let link = st.ifaces.link(index).ok_or(Errno::ENODEV)?;
        if link.kind.name().is_none() {
            return Err(Errno::EOPNOTSUPP);
        }
        let link = st.ifaces.remove(index)?;
        drop(st);
        // `veth_dellink`: the peer goes too, wherever it is.
        if let LinkKind::Veth(end) = &link.kind {
            let (net, peer) = end.peer();
            if let Some(net) = net.upgrade() {
                let mut st = net.configure(&rtnl);
                let is_peer = matches!(
                    st.ifaces.link(peer).map(|l| &l.kind),
                    Some(LinkKind::Veth(other)) if other.is_peer_of(end)
                );
                if is_peer {
                    let _ = st.ifaces.remove(peer);
                }
            }
        }
        Ok(())
    }

    fn rename_link(&self, index: u32, name: &str) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.rename(index, name)
    }

    fn set_mtu(&self, index: u32, mtu: u32) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.set_mtu(index, mtu)
    }

    fn set_mac(&self, index: u32, mac: [u8; 6]) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.set_mac(index, mac)
    }

    fn add_address(&self, index: u32, addr: IfAddr, mode: AddMode) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.add_addr(index, addr, mode)
    }

    fn del_address(&self, index: u32, which: &AddrMatch) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.del_addr(index, which)
    }

    fn add_route(&self, route: Route, mode: AddMode) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.add_route(route, mode)
    }

    fn del_route(&self, which: &RouteMatch) -> Result<(), Errno> {
        let rtnl = rtnl();
        self.configure(&rtnl).ifaces.del_route(which)
    }

    fn take_link(&self, rtnl: &Rtnl, index: u32) -> Result<MovedLink, Errno> {
        let mut st = self.configure(rtnl);
        let link = st.ifaces.link(index).ok_or(Errno::ENODEV)?;
        if link.kind.netns_local() {
            return Err(Errno::EINVAL);
        }
        let mut link = st.ifaces.remove(index)?;
        // `ip_mc_destroy_dev`: an interface leaves its groups as it leaves the namespace.
        link.joined.clear();
        link.filters.clear();
        Ok(MovedLink {
            link: Box::new(link),
        })
    }

    fn adopt_link(&self, rtnl: &Rtnl, moved: MovedLink) -> Result<u32, (Errno, MovedLink)> {
        let mut st = self.configure(rtnl);
        if st.ifaces.link_by_name(&moved.link.name).is_some() {
            return Err((Errno::EEXIST, moved));
        }
        let mut link = *moved.link;
        link.index = st.ifaces.reserve_index(Some(link.index));
        if let LinkKind::Veth(end) = &link.kind {
            end.moved(self.me.clone(), link.index);
        }
        let index = link.index;
        st.ifaces.insert(link);
        Ok(index)
    }
}

/// Hands backend connections to whichever guest listener holds `guest` at the time.
struct GuestListenerSink {
    net: Weak<Net>,
    guest: SocketAddr,
}

impl GuestListenerSink {
    /// The guest address a connection from `peer` arrives at: the published one, or for a
    /// wildcard the uplink address (or loopback) the guest sees it on.
    fn destination(&self, net: &Net, peer: SocketAddr) -> SocketAddr {
        net.inbound_destination(&net.state(), self.guest, Some(peer))
    }
}

impl InboundSink for GuestListenerSink {
    fn poll_room(&self, table: Option<&mut PollTable<'_>>) -> Room {
        let Some(net) = self.net.upgrade() else {
            return Room::Gone;
        };
        // The peer is not known before the host accepts; the listener a wildcard
        // publication reaches does not depend on it beyond the address family.
        let any_peer = match self.guest {
            SocketAddr::V4(_) => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketAddr::V6(_) => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };
        let dst = self.destination(&net, any_peer);
        let mut table = table;
        let mut room = Room::Gone;
        for listener in net.listener_group(dst, any_peer) {
            match listener.poll_room(table.as_deref_mut()) {
                Room::Ready => return Room::Ready,
                Room::Full => room = Room::Full,
                Room::Gone => {}
            }
        }
        room
    }

    fn incoming(&self, stream: Arc<dyn BackendStream>, wq: WaitQueue) -> bool {
        let Some(net) = self.net.upgrade() else {
            return false;
        };
        let peer = net.guest_view(stream.peer_addr());
        let dst = self.destination(&net, peer);
        // The listener the peer hashes to, else another of its group with room.
        let mut stream = stream;
        for listener in net.listener_group(dst, peer) {
            match listener.incoming_remote(stream, wq.clone(), dst, peer) {
                Offer::Queued => return true,
                Offer::Dropped => return false,
                Offer::Declined(back) => stream = back,
            }
        }
        false
    }
}
