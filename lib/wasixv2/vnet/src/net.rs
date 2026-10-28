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
    Backend, BackendDatagram, BackendStream, Domain, InboundSink, Interface, NetResult, Network,
    PortEntry, Publication, Room, Socket, SocketKind,
    addr::{canonical, canonical_ip},
    cmsg::SendCtl,
    iface::{Interfaces, Route, RouteDecision},
    ports::{BindSpec, PortRange, PortRng, PortTable, Proto, SockId},
    tcp::{Offer, TcpSocket},
    udp::UdpSocket,
};

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
            seed: 0x5eed,
        }
    }
}

impl NetConfig {
    /// A namespace behind a host backend: loopback plus the default uplink, with the gateway
    /// reaching the host's loopback.
    pub fn host_default() -> Self {
        let uplink = Uplink::default();
        let gw = uplink.v4.and_then(|v| v.2).map(IpAddr::V4);
        Self {
            uplink: Some(uplink),
            host_loopback_alias: gw,
            ..Self::default()
        }
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
    Local { src: IpAddr },
    Backend { src: IpAddr, host_dst: SocketAddr },
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
            let idx = ifaces.add_link(&up.name, up.mtu, up.mac);
            ifaces.set_up(idx, true).expect("link exists");
            if let Some((a, len, gw)) = up.v4 {
                ifaces
                    .add_addr(idx, a.into(), len)
                    .expect("valid uplink address");
                if let Some(gw) = gw {
                    ifaces
                        .add_route(default_route(Ipv4Addr::UNSPECIFIED.into(), gw.into(), idx))
                        .expect("fresh table");
                }
            }
            if let Some((a, len, gw)) = up.v6 {
                ifaces
                    .add_addr(idx, a.into(), len)
                    .expect("valid uplink address");
                if let Some(gw) = gw {
                    ifaces
                        .add_route(default_route(Ipv6Addr::UNSPECIFIED.into(), gw.into(), idx))
                        .expect("fresh table");
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
            }),
            backend,
            config,
            next_id: AtomicU64::new(1),
            me: me.clone(),
        })
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, NetState> {
        self.state.lock().unwrap()
    }

    pub(crate) fn arc(&self) -> Arc<Net> {
        self.me.upgrade().expect("network alive while used")
    }

    pub(crate) fn next_id(&self) -> SockId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Changes the interface and route tables (`ip link`, `ip addr`, `ip route`).
    pub fn update_interfaces<R>(&self, f: impl FnOnce(&mut Interfaces) -> R) -> R {
        f(&mut self.state().ifaces)
    }

    /// `fib_lookup` for `dst`.
    pub fn route(&self, dst: IpAddr) -> Option<RouteDecision> {
        self.state().ifaces.route(dst)
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
        self.path_in(&self.state(), dst, bound)
    }

    fn path_in(&self, st: &NetState, dst: SocketAddr, bound: Option<IpAddr>) -> NetResult<Path> {
        let decision = st.ifaces.route(dst.ip()).ok_or(Errno::ENETUNREACH)?;
        let src = bound
            .filter(|ip| !ip.is_unspecified())
            .unwrap_or(decision.src);
        if decision.local {
            return Ok(Path::Local { src });
        }
        if self.backend.is_none() {
            return Err(Errno::ENETUNREACH);
        }
        Ok(Path::Backend {
            src,
            host_dst: self.host_view(dst),
        })
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
        let route = {
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
            let path = self.path_in(&st, dst, src.or(Some(local.ip())))?;
            match path {
                Path::Local { src } => {
                    let from = SocketAddr::new(src, local.port());
                    // Liveness without upgrading: a last reference must not drop under the lock.
                    let live = |w: &Weak<UdpSocket>, _| w.strong_count() > 0;
                    let to = st
                        .udp
                        .lookup_one(dst, Some(from), live, from.port() as usize);
                    Ok((from, to))
                }
                Path::Backend { host_dst, .. } => Err(host_dst),
            }
        };
        Ok(match route {
            Ok((from, to)) => UdpRoute::Local {
                from,
                to: to.and_then(|w| w.upgrade()),
            },
            Err(host_dst) => UdpRoute::Backend { host_dst },
        })
    }
}

/// Whether a UDP socket bound to `local` receives datagrams for `dst` (`compute_score`'s
/// address match: the same address, or a wildcard of a family that takes it).
fn serves(local: SocketAddr, v6only: bool, dst: SocketAddr) -> bool {
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
    },
    /// Out through the backend to `host_dst`.
    Backend { host_dst: SocketAddr },
}

fn default_route(dst: IpAddr, gw: IpAddr, oif: u32) -> Route {
    Route {
        dst,
        prefix_len: 0,
        gateway: Some(gw),
        oif,
        prefsrc: None,
        metric: 0,
    }
}

impl Network for Net {
    fn socket(
        &self,
        domain: Domain,
        kind: SocketKind,
        protocol: i32,
        uid: u32,
    ) -> Result<Arc<dyn Socket>, Errno> {
        // `inet_create`: 0 means the default; IPPROTO_TCP=6, IPPROTO_UDP=17.
        match (kind, protocol) {
            (SocketKind::Stream, 0 | 6) => Ok(TcpSocket::new(self.arc(), domain, uid)),
            (SocketKind::Datagram, 0 | 17) => Ok(UdpSocket::new(self.arc(), domain, uid)),
            _ => Err(Errno::EPROTONOSUPPORT),
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

    fn interfaces(&self) -> Vec<Interface> {
        self.state()
            .ifaces
            .links()
            .iter()
            .map(|l| Interface {
                index: l.index,
                name: l.name.clone(),
                flags: l.flags,
                mtu: l.mtu,
                addresses: l.addrs.iter().map(|a| (a.addr, a.prefix_len)).collect(),
            })
            .collect()
    }

    fn change_flags(&self, index: u32, flags: u32) -> Result<(), Errno> {
        self.state().ifaces.change_flags(index, flags)
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
