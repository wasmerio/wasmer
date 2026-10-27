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
    Backend, BackendStream, Domain, InboundSink, Interface, NetResult, Network, PortEntry,
    Publication, Room, Socket, SocketKind,
    addr::canonical,
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
    /// Auto-publishing (opt-in): mirror each guest listener on this host address, same port.
    /// Listeners bound only to a loopback address stay private.
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
    pub ifaces: Interfaces,
    pub rng: PortRng,
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

    /// Where a UDP datagram to `dst` goes, decided under one lock: the route, the sender's port
    /// (bound with `autobind` first if `local` is `None` and there is a route), and inside the
    /// namespace the receiving socket. Returns the sender's local address and the route.
    pub(crate) fn udp_route(
        &self,
        id: SockId,
        autobind: BindSpec,
        local: Option<SocketAddr>,
        dst: SocketAddr,
        me: Weak<UdpSocket>,
    ) -> NetResult<(SocketAddr, UdpRoute)> {
        let (local, route) = {
            let mut st = self.state();
            let st = &mut *st;
            let path = self.path_in(st, dst, local.map(|l| l.ip()))?;
            let local = match local {
                Some(local) => local,
                None => {
                    let range = self.config.port_range;
                    let port = st.udp.bind(id, autobind, 0, range, &mut st.rng, me)?;
                    SocketAddr::new(autobind.addr, port)
                }
            };
            let route = match path {
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
            };
            (local, route)
        };
        let route = match route {
            Ok((from, to)) => UdpRoute::Local {
                from,
                to: to.and_then(|w| w.upgrade()),
            },
            Err(host_dst) => UdpRoute::Backend { host_dst },
        };
        Ok((local, route))
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
        let mut dst = self.guest;
        if dst.ip().is_unspecified() {
            // The guest sees the connection arrive on its uplink address, or loopback.
            let ip = net
                .state()
                .ifaces
                .route(peer.ip())
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
