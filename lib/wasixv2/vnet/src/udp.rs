//! UDP sockets: datagrams move directly between the receive queues of a namespace's sockets;
//! datagrams for other destinations go through a backend datagram endpoint.

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use wasixv2_abi::{
    Errno, GuestLayout, IpOption, Ipv6Option, MsgFlags, SoEeOrigin, SockExtendedErr, SocketLevel,
    SocketOption,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, BackendDatagram, Control, Domain, NetResult, PortEntry, Publication, QueuedError,
    RecvMsg, SendMsg, Shutdown, Socket, SocketKind,
    addr::{canonical, canonical_ip, present, sockaddr_bytes, unbound},
    cmsg::{self, SendCtl},
    mcast::McRoute,
    net::{Net, Path, UdpRoute, serves},
    opts::{Changed, SockOpts},
    ports::{BindSpec, SockId},
};

/// What a queued datagram is charged against `SO_RCVBUF` on top of its payload, roughly the
/// `truesize` overhead of a small skb on 64-bit Linux.
const DGRAM_OVERHEAD: usize = 768;

/// The most `udp_sendmsg` takes at all, before the IP layer's own limit below.
const MAX_SEND: usize = 0xFFFF;
/// Largest UDP payload: 65535 minus the IP and UDP headers.
const MAX_V4_PAYLOAD: usize = 65507;
const MAX_V6_PAYLOAD: usize = 65527;

/// How much of a refused datagram's payload a port unreachable quotes. `icmp_send` quotes at
/// most 576 bytes of IP packet less its own IP and ICMP headers, of which the quoted IP and UDP
/// headers take 28; `icmp6_send` quotes up to the IPv6 minimum MTU less its 48 bytes of
/// headers, of which the quoted ones take another 48.
const ICMP_QUOTE: usize = 576 - 20 - 8 - 20 - 8;
const ICMP6_QUOTE: usize = 1280 - 40 - 8 - 40 - 8;
/// `ICMP_DEST_UNREACH`/`ICMP_PORT_UNREACH` and `ICMPV6_DEST_UNREACH`/`ICMPV6_PORT_UNREACH`.
const ICMP_PORT_UNREACH: (u8, u8) = (3, 3);
const ICMPV6_PORT_UNREACH: (u8, u8) = (1, 4);
/// The options that make ICMP errors queue (`inet->recverr`, `np->recverr`).
const RECVERR: [(i32, i32); 2] = [
    (SocketLevel::SOL_IP.0, IpOption::IP_RECVERR.0),
    (SocketLevel::SOL_IPV6.0, Ipv6Option::IPV6_RECVERR.0),
];

struct Datagram {
    data: Vec<u8>,
    /// Canonical sender.
    from: SocketAddr,
}

struct Inner {
    opts: SockOpts,
    local: Option<SocketAddr>,
    /// `inet_rcv_saddr` of a bind that took no port (`IP_BIND_ADDRESS_NO_PORT`): the address
    /// the port taken later by `connect` or a send goes with.
    saddr: Option<IpAddr>,
    /// `SOCK_BINDADDR_LOCK` / `SOCK_BINDPORT_LOCK`.
    addr_locked: bool,
    port_locked: bool,
    peer: Option<SocketAddr>,
    queue: VecDeque<Datagram>,
    /// `sk_rmem_alloc`, which the error queue is charged against too.
    queued: usize,
    err: Option<Errno>,
    /// `sk_error_queue`: errors of datagrams sent inside the namespace.
    errqueue: VecDeque<QueuedError>,
    rd_shut: bool,
    wr_shut: bool,
    /// The endpoint for traffic leaving the namespace, while the socket has one.
    backend: Option<Arc<dyn BackendDatagram>>,
    /// The publication the socket serves (its id and endpoint), whose endpoint is `backend`
    /// unless the socket is connected inside the namespace.
    published: Option<(u64, Arc<dyn BackendDatagram>)>,
}

/// A UDP socket of a [`Net`].
pub struct UdpSocket {
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    /// `sk_uid`, for `SO_REUSEPORT`.
    uid: u32,
    /// `sock_i_ino`: the inode of the socket's file, 0 until it has one.
    ino: AtomicU64,
    wq: WaitQueue,
    me: Weak<UdpSocket>,
    inner: Mutex<Inner>,
    /// The auto-publication of the bound port, for as long as the socket lives.
    auto_publication: Mutex<Option<Box<dyn Publication>>>,
}

impl UdpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain, uid: u32) -> Arc<Self> {
        let id = net.next_id();
        Arc::new_cyclic(|me| Self {
            net,
            id,
            domain,
            uid,
            ino: AtomicU64::new(0),
            wq: WaitQueue::new(),
            me: me.clone(),
            inner: Mutex::new(Inner {
                opts: SockOpts::new(SocketKind::Datagram, domain),
                local: None,
                saddr: None,
                addr_locked: false,
                port_locked: false,
                peer: None,
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                errqueue: VecDeque::new(),
                rd_shut: false,
                wr_shut: false,
                backend: None,
                published: None,
            }),
            auto_publication: Mutex::new(None),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    fn spec(&self, o: &SockOpts, ip: IpAddr) -> BindSpec {
        BindSpec {
            family: self.domain,
            addr: ip,
            v6only: o.v6only,
            reuseaddr: o.reuseaddr,
            reuseport: o.reuseport,
            uid: self.uid,
        }
    }

    fn any(&self) -> IpAddr {
        match self.domain {
            Domain::Inet => Ipv4Addr::UNSPECIFIED.into(),
            Domain::Inet6 => std::net::Ipv6Addr::UNSPECIFIED.into(),
        }
    }

    fn autobind(&self, g: &mut Inner) -> NetResult<SocketAddr> {
        if let Some(local) = g.local {
            return Ok(local);
        }
        let ip = g.saddr.unwrap_or(self.any());
        let port = self
            .net
            .udp_bind(self.id, self.spec(&g.opts, ip), 0, self.me.clone())?;
        let local = SocketAddr::new(ip, port);
        g.local = Some(local);
        Ok(local)
    }

    pub(crate) fn port_entry(&self) -> Option<PortEntry> {
        let g = self.lock();
        Some(PortEntry {
            kind: SocketKind::Datagram,
            local: present(self.domain, g.local?),
            peer: g.peer.map(|p| present(self.domain, p)),
            listening: false,
            uid: self.uid,
            inode: self.ino.load(Ordering::Relaxed),
        })
    }

    /// `__udp_enqueue_schedule_skb`: drop when the queue already exceeds `SO_RCVBUF`.
    fn deliver(&self, data: &[u8], from: SocketAddr) {
        let mut g = self.lock();
        if g.queued > g.opts.rcvbuf {
            return;
        }
        g.queued += data.len() + DGRAM_OVERHEAD;
        g.queue.push_back(Datagram {
            data: data.to_vec(),
            from,
        });
        drop(g);
        self.wq.wake(PollMask::IN | PollMask::RDNORM);
    }

    /// The endpoint for traffic leaving the namespace, made on first use. It wakes this
    /// socket's queue, so waiters registered before it existed hear its datagrams.
    fn backend_endpoint(&self, g: &mut Inner) -> NetResult<Arc<dyn BackendDatagram>> {
        if let Some(b) = &g.backend {
            return Ok(b.clone());
        }
        if let Some((_, ep)) = g.published.clone() {
            ep.attach(Some(self.wq.clone()))?;
            Self::forward_options(&g.opts, &*ep);
            g.backend = Some(ep.clone());
            return Ok(ep);
        }
        let backend = self.net.backend.as_ref().ok_or(Errno::ENETUNREACH)?;
        let b = backend.datagram(self.domain, self.wq.clone())?;
        Self::forward_options(&g.opts, &*b);
        g.backend = Some(b.clone());
        Ok(b)
    }

    /// Takes a publication that reaches the socket's bound address, if one is free (`id`, or
    /// any). The socket's lock is held.
    fn claim_publication(&self, g: &mut Inner, id: Option<u64>) -> bool {
        let Some(local) = g.local.filter(|_| g.published.is_none()) else {
            return false;
        };
        let sock = (self.id, self.me.clone());
        let Some((id, ep)) = self
            .net
            .claim_udp_publication(local, g.opts.v6only, sock, id)
        else {
            return false;
        };
        g.published = Some((id, ep));
        self.activate_publication(g);
        true
    }

    /// Offered publication `id` by the network: takes it if it reaches the bound address.
    pub(crate) fn adopt_publication(&self, id: u64) -> bool {
        self.claim_publication(&mut self.lock(), Some(id))
    }

    /// Publication `id` ended.
    pub(crate) fn lose_publication(&self, id: u64) {
        let mut g = self.lock();
        let Some((held, ep)) = g.published.take() else {
            return;
        };
        if held != id {
            g.published = Some((held, ep));
            return;
        }
        if g.backend.as_ref().is_some_and(|b| Arc::ptr_eq(b, &ep)) {
            g.backend = None;
        }
        drop(g);
        let _ = ep.attach(None);
    }

    /// Gives up the publication when the socket unbinds; the caller offers it on once no
    /// socket lock is held.
    fn release_publication(&self, g: &mut Inner) -> Option<u64> {
        let (id, ep) = g.published.take()?;
        if g.backend.as_ref().is_some_and(|b| Arc::ptr_eq(b, &ep)) {
            g.backend = None;
        }
        let _ = ep.attach(None);
        self.net.release_udp_publication(id, self.id);
        Some(id)
    }

    /// Makes the published endpoint serve the socket as fits its peer: unconnected, or
    /// connected outside the namespace (the endpoint then connected to the host's view of the
    /// peer), it is the socket's way in and out; connected inside the namespace, nothing from
    /// outside is for the socket, so the endpoint drops what arrives.
    fn activate_publication(&self, g: &mut Inner) {
        let Some((_, ep)) = g.published.clone() else {
            return;
        };
        let host_peer = match g.peer.map(|p| self.net.path(p, g.local.map(|l| l.ip()))) {
            None => None,
            Some(Ok(Path::Backend { host_dst, .. })) => Some(host_dst),
            Some(_) => {
                let _ = ep.attach(None);
                return;
            }
        };
        if let Err(e) = ep.attach(Some(self.wq.clone())) {
            tracing::warn!(error = ?e, "attaching a published UDP endpoint");
            return;
        }
        Self::forward_options(&g.opts, &*ep);
        if let Some(peer) = host_peer
            && let Err(e) = ep.connect(Some(peer))
        {
            tracing::debug!(error = ?e, "connecting a published UDP endpoint");
        }
        g.backend = Some(ep);
    }

    /// Auto-publishing: mirror an explicitly bound, non-loopback socket's port on the host.
    /// Failure is logged only.
    fn auto_publish(&self, local: SocketAddr) {
        let (Some(host_ip), Some(_)) = (self.net.config.auto_publish, self.net.backend.as_ref())
        else {
            return;
        };
        if local.ip().is_loopback() || self.lock().published.is_some() {
            return;
        }
        let host = SocketAddr::new(host_ip, local.port());
        match self.net.publish_udp(host, local) {
            Ok(p) => *self.auto_publication.lock().unwrap() = Some(p),
            Err(e) => tracing::warn!(%host, guest = %local, error = %e, "auto-publish failed"),
        }
    }

    /// Hands the options a backend endpoint acts on to one made after they were set.
    fn forward_options(opts: &SockOpts, b: &dyn BackendDatagram) {
        for (level, name) in RECVERR {
            if opts.flag(level, name) {
                Self::forward_option(b, level, name, &1i32.to_le_bytes());
            }
        }
    }

    fn forward_option(b: &dyn BackendDatagram, level: i32, name: i32, value: &[u8]) {
        if let Err(e) = b.set_option(level, name, value) {
            tracing::debug!(level, name, error = ?e, "backend datagram option not applied");
        }
    }

    /// An ICMP port unreachable for a datagram sent inside the namespace to `dst`, where no
    /// socket is bound (`__udp4_lib_err`, `__udp6_lib_err`): with `IP_RECVERR` (or
    /// `IPV6_RECVERR` for an IPv6 destination) it is queued and becomes the pending error;
    /// without, only a connected socket hears of it.
    fn port_unreachable(&self, dst: SocketAddr, payload: &[u8]) {
        let mut g = self.lock();
        let (level_name, origin, (type_, code), quote) = match dst {
            SocketAddr::V4(_) => (
                RECVERR[0],
                SoEeOrigin::SO_EE_ORIGIN_ICMP,
                ICMP_PORT_UNREACH,
                ICMP_QUOTE,
            ),
            SocketAddr::V6(_) => (
                RECVERR[1],
                SoEeOrigin::SO_EE_ORIGIN_ICMP6,
                ICMPV6_PORT_UNREACH,
                ICMP6_QUOTE,
            ),
        };
        if g.opts.flag(level_name.0, level_name.1) {
            let e = QueuedError {
                errno: Errno::ECONNREFUSED,
                origin,
                type_,
                code,
                info: 0,
                data: 0,
                // The destination itself answers.
                offender: Some(SocketAddr::new(dst.ip(), 0)),
                dst: Some(dst),
                payload: payload[..payload.len().min(quote)].to_vec(),
            };
            Self::queue_error(&mut g, e);
        } else if g.peer.is_none() {
            return;
        }
        g.err = Some(Errno::ECONNREFUSED);
        drop(g);
        self.wq.wake(PollMask::ERR);
    }

    /// `sock_queue_err_skb`: dropped when the receive buffer is full.
    fn queue_error(g: &mut Inner, e: QueuedError) {
        let size = e.payload.len() + DGRAM_OVERHEAD;
        if g.queued + size >= g.opts.rcvbuf {
            return;
        }
        g.queued += size;
        g.errqueue.push_back(e);
    }

    /// `sock_dequeue_err_skb`: the oldest local entry, leaving the next ICMP entry's errno, or
    /// none, as the pending error.
    fn dequeue_error(g: &mut Inner) -> Option<QueuedError> {
        let e = g.errqueue.pop_front()?;
        g.queued -= e.payload.len() + DGRAM_OVERHEAD;
        match g.errqueue.front() {
            Some(next) if next.is_icmp() => g.err = Some(next.errno),
            _ if e.is_icmp() => g.err = None,
            _ => {}
        }
        Some(e)
    }

    /// `ip_recv_error` / `ipv6_recv_error`: one entry of the error queue, never waiting.
    fn recv_error(&self, buf: &mut [u8]) -> NetResult<RecvMsg> {
        let mut g = self.lock();
        let e = match Self::dequeue_error(&mut g) {
            Some(e) => {
                let more = !g.errqueue.is_empty();
                drop(g);
                if more {
                    self.wq.wake(PollMask::ERR);
                }
                e
            }
            None => {
                let backend = g.backend.clone();
                drop(g);
                let mut e = backend
                    .and_then(|b| b.take_queued_error())
                    .ok_or(Errno::EAGAIN)?;
                e.dst = e.dst.map(|a| self.net.guest_view(a));
                e.offender = e.offender.map(|a| self.net.guest_view(a));
                e
            }
        };
        Ok(error_result(self.domain, &e, buf))
    }

    /// `sock_error`: the socket's pending error, else the backend endpoint's.
    fn take_err(g: &mut Inner) -> Option<Errno> {
        g.err
            .take()
            .or_else(|| g.backend.as_ref().and_then(|b| b.take_error()))
    }
}

impl UdpSocket {
    /// The interface and source address of a datagram to the group `dst`
    /// (`udp_sendmsg`, `udpv6_sendmsg`, `ip4_datagram_connect`): the interface the control
    /// messages name, else the destination's scope, the socket's device, then
    /// `IP_MULTICAST_IF`; the source the control messages name, else the bound address (not a
    /// group or broadcast address, which leave the source to the device), then
    /// `IP_MULTICAST_IF`'s.
    fn mc_route(
        &self,
        g: &Inner,
        dst: SocketAddr,
        scope: u32,
        ctl: Option<&SendCtl>,
    ) -> NetResult<McRoute> {
        let group = dst.ip();
        let (mc_oif, mc_src) = g.opts.mc.interface(group);
        let ctl_oif = ctl.map_or(0, |c| c.oif);
        let oif = [
            ctl_oif,
            scope,
            self.net.device_index(&g.opts.bind_device),
            mc_oif,
        ]
        .into_iter()
        .find(|&i| i != 0)
        .unwrap_or(0);
        let bound = g
            .local
            .map(|l| l.ip())
            .or(g.saddr)
            .filter(|ip| ip.is_ipv4() == group.is_ipv4())
            .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
            .filter(|ip| *ip != IpAddr::V4(Ipv4Addr::BROADCAST));
        let src = ctl
            .and_then(|c| c.src)
            .map(canonical_ip)
            .or(bound)
            .or(mc_src);
        self.net.mc_route(group, oif, src)
    }

    /// A send to the group `dst`, which each socket of the namespaces it reaches that is
    /// bound to the port and allows the group gets a copy of (`__udp4_lib_mcast_deliver`,
    /// `__udp6_lib_mcast_deliver`).
    fn send_multicast(
        &self,
        mut g: MutexGuard<'_, Inner>,
        local: SocketAddr,
        dst: SocketAddr,
        scope: u32,
        ctl: &SendCtl,
        data: &[u8],
    ) -> NetResult<usize> {
        let route = self.mc_route(&g, dst, scope, Some(ctl))?;
        let max = match dst.is_ipv4() {
            true => MAX_V4_PAYLOAD,
            false => MAX_V6_PAYLOAD,
        };
        if data.len() > max {
            return Err(Errno::EMSGSIZE);
        }
        if let Some(size) = ctl.gso_size {
            cmsg::check_segments(data.len(), size)?;
        }
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        let (ttl, looped) = g.opts.mc.ttl_loop(dst.ip());
        let ttl = ctl.ttl.unwrap_or(ttl);
        drop(g);
        let from = SocketAddr::new(route.src, local.port());
        let segment = ctl.gso_size.unwrap_or(usize::MAX).max(1);
        for (net, iif) in self.net.mc_receivers(dst.ip(), route.oif, looped, ttl) {
            if data.is_empty() {
                net.udp_mcast_deliver(dst, from, iif, data);
            }
            for chunk in data.chunks(segment) {
                net.udp_mcast_deliver(dst, from, iif, chunk);
            }
        }
        Ok(data.len())
    }

    /// `__udp_is_mcast_sock`, `__udp_v6_is_mcast_sock`: takes a datagram to the group `dst`
    /// from `from` that arrived on interface `iif` if the socket is bound to the port and to
    /// the group or a wildcard of its family, connected to nobody else, on no other device,
    /// and allows the group (`ip_mc_sf_allow`, `inet6_mc_check`).
    fn deliver_multicast(&self, dst: SocketAddr, from: SocketAddr, iif: u32, data: &[u8]) {
        let g = self.lock();
        let Some(local) = g.local else {
            return;
        };
        let dev = self.net.device_index(&g.opts.bind_device);
        let takes = serves(local, g.opts.v6only, dst)
            && g.peer.is_none_or(|p| p == from)
            && (dev == 0 || dev == iif)
            && g.opts.mc.allows(dst.ip(), iif);
        drop(g);
        if takes {
            self.deliver(data, from);
        }
    }
}

impl Net {
    /// A datagram to the group `dst` arrived on interface `iif`: every socket bound to its
    /// port is offered it.
    pub(crate) fn udp_mcast_deliver(
        &self,
        dst: SocketAddr,
        from: SocketAddr,
        iif: u32,
        data: &[u8],
    ) {
        let socks = self.state().udp_on_port(dst.port());
        for sock in socks.iter().filter_map(Weak::upgrade) {
            sock.deliver_multicast(dst, from, iif, data);
        }
    }
}

/// The interface a link-scoped IPv6 destination names (`sin6_scope_id`), 0 for none.
fn scope_of(addr: SocketAddr) -> u32 {
    match addr {
        SocketAddr::V6(v6) => v6.scope_id(),
        SocketAddr::V4(_) => 0,
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let g = self.inner.get_mut().unwrap();
        g.opts.mc.drop_all(&self.net);
        let local = g.local.take();
        let published = g.published.take();
        if let Some(local) = local {
            self.net.state().udp.unbind(self.id, local.port());
        }
        // The next socket bound where the publication reaches takes it over.
        if let Some((id, ep)) = published {
            let _ = ep.attach(None);
            self.net.release_udp_publication(id, self.id);
            self.net.offer_udp_publication(id);
        }
    }
}

impl Pollable for UdpSocket {
    /// `udp_poll` / `datagram_poll`.
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
        // `sock_writeable`: datagrams inside the namespace never wait; the backend's endpoint
        // is writable while the host would take a datagram.
        let mut writable = true;
        if let Some(b) = backend {
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

impl Socket for UdpSocket {
    fn domain(&self) -> Domain {
        self.domain
    }

    fn kind(&self) -> SocketKind {
        SocketKind::Datagram
    }

    fn protocol(&self) -> i32 {
        17
    }

    fn bind(&self, addr: SocketAddr, privileged: bool) -> NetResult<()> {
        let mapped = matches!(addr, SocketAddr::V6(v6) if v6.ip().to_ipv4_mapped().is_some());
        let addr = canonical(addr);
        let mut g = self.lock();
        if mapped && g.opts.v6only {
            return Err(Errno::EINVAL);
        }
        let freebind = g.opts.flag(0, wasixv2_abi::IpOption::IP_FREEBIND.0);
        self.net.check_bind(addr, privileged, freebind)?;
        if g.local.is_some() {
            return Err(Errno::EINVAL);
        }
        // `__inet_bind`: without a port, the address waits for the port autobind picks.
        if addr.port() == 0 && g.opts.flag(0, IpOption::IP_BIND_ADDRESS_NO_PORT.0) {
            g.saddr = Some(addr.ip());
            g.addr_locked |= !addr.ip().is_unspecified();
            return Ok(());
        }
        let port = self.net.udp_bind(
            self.id,
            self.spec(&g.opts, addr.ip()),
            addr.port(),
            self.me.clone(),
        )?;
        let local = SocketAddr::new(addr.ip(), port);
        g.local = Some(local);
        g.saddr = None;
        g.addr_locked |= !addr.ip().is_unspecified();
        g.port_locked = addr.port() != 0;
        self.claim_publication(&mut g, None);
        drop(g);
        self.auto_publish(local);
        Ok(())
    }

    fn listen(&self, _backlog: u32) -> NetResult<()> {
        Err(Errno::EOPNOTSUPP)
    }

    fn accept(&self) -> NetResult<Accepted> {
        Err(Errno::EOPNOTSUPP)
    }

    /// `ip4_datagram_connect`: sets the default peer, binds if needed, and takes the route's
    /// source address if bound to the wildcard.
    fn connect(&self, addr: SocketAddr) -> NetResult<()> {
        let dst = canonical(addr);
        let mut g = self.lock();
        let path = match dst.ip().is_multicast() {
            // `ip4_datagram_connect` routes a group by `IP_MULTICAST_IF`.
            true => {
                let route = self.mc_route(&g, dst, scope_of(addr), None)?;
                Path::Nowhere { src: route.src }
            }
            false => self.net.path(dst, g.local.map(|l| l.ip()).or(g.saddr))?,
        };
        let src = path.src();
        let local = self.autobind(&mut g)?;
        if local.ip().is_unspecified() {
            self.net.state().udp.rehash(self.id, local.port(), src);
            g.local = Some(SocketAddr::new(src, local.port()));
        }
        self.net
            .state()
            .udp
            .set_remote(self.id, local.port(), Some(dst));
        match path {
            // The host endpoint filters by the peer and reports its ICMP errors.
            Path::Backend { host_dst, .. } => {
                self.backend_endpoint(&mut g)?.connect(Some(host_dst))?
            }
            // The association moved inside the namespace or to a link of its own: nothing from
            // outside is for us now.
            Path::Local { .. } | Path::Nowhere { .. } => {
                g.backend = None;
                if let Some((_, ep)) = &g.published {
                    let _ = ep.attach(None);
                }
            }
        }
        g.peer = Some(dst);
        Ok(())
    }

    /// `udp_disconnect`.
    fn disconnect(&self) -> NetResult<()> {
        let mut g = self.lock();
        g.peer = None;
        if let Some(b) = &g.backend {
            b.connect(None)?;
        }
        let mut released = None;
        if let Some(local) = g.local {
            let mut st = self.net.state();
            st.udp.set_remote(self.id, local.port(), None);
            if !g.port_locked {
                st.udp.unbind(self.id, local.port());
                g.local = None;
                // An address `bind` chose stays without the port (`inet_reset_saddr` only
                // for an unlocked one).
                g.saddr = g.addr_locked.then_some(local.ip());
                drop(st);
                released = self.release_publication(&mut g);
            } else if !g.addr_locked {
                st.udp.rehash(self.id, local.port(), self.any());
                g.local = Some(SocketAddr::new(self.any(), local.port()));
            }
        }
        // Unconnected again, a socket whose publication was detached while connected inside
        // the namespace hears from outside again; one connected outside kept its queue.
        if g.published.is_some() && g.backend.is_none() {
            self.activate_publication(&mut g);
        }
        drop(g);
        if let Some(id) = released {
            self.net.offer_udp_publication(id);
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
        // `inet_shutdown` on an unconnected socket records the shutdown but says ENOTCONN.
        if connected {
            Ok(())
        } else {
            Err(Errno::ENOTCONN)
        }
    }

    /// `inet_sendmsg` and `udp_sendmsg`/`udpv6_sendmsg`, failing in their order: autobind,
    /// the size and flags, the destination, the control messages, the route, the payload
    /// limit, then (`sock_alloc_send_pskb`) a pending error and a shut down sender.
    fn send(&self, msg: SendMsg<'_>) -> NetResult<usize> {
        let mut g = self.lock();
        // `inet_send_prepare`: an unbound socket is bound first, `EAGAIN` if that fails.
        let local = self.autobind(&mut g).map_err(|_| Errno::EAGAIN)?;
        if msg.data.len() > MAX_SEND {
            return Err(Errno::EMSGSIZE);
        }
        if msg.flags.0 & MsgFlags::MSG_OOB.0 != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        let dst = match msg.to.map(canonical) {
            Some(d) if d.port() == 0 => return Err(Errno::EINVAL),
            Some(d) => d,
            None => g.peer.ok_or(Errno::EDESTADDRREQ)?,
        };
        // `udpv6_sendmsg`: an IPv4 destination needs a dual-stack socket.
        if self.domain == Domain::Inet6 && dst.is_ipv4() && g.opts.v6only {
            return Err(Errno::ENETUNREACH);
        }
        let ctl = cmsg::udp(msg.control, self.domain, dst.is_ipv4(), msg.net_raw)?;
        if let IpAddr::V4(v4) = dst.ip()
            && v4.is_broadcast()
        {
            // Broadcast is not carried (TODO); Linux still checks SO_BROADCAST.
            if !g
                .opts
                .flag(SocketLevel::SOL_SOCKET.0, SocketOption::SO_BROADCAST.0)
            {
                return Err(Errno::EACCES);
            }
            return Ok(msg.data.len());
        }
        if dst.ip().is_multicast() {
            let scope = msg.to.map_or(0, scope_of);
            return self.send_multicast(g, local, dst, scope, &ctl, msg.data);
        }
        let route = self.net.udp_route(local, dst, &ctl)?;
        let max = if dst.is_ipv4() {
            MAX_V4_PAYLOAD
        } else {
            MAX_V6_PAYLOAD
        };
        if msg.data.len() > max {
            return Err(Errno::EMSGSIZE);
        }
        if let Some(size) = ctl.gso_size {
            cmsg::check_segments(msg.data.len(), size)?;
        }
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if g.wr_shut {
            return Err(Errno::EPIPE);
        }
        // `UDP_SEGMENT`: the payload leaves as datagrams of the segment size.
        let segment = ctl.gso_size.unwrap_or(usize::MAX).max(1);
        match route {
            UdpRoute::Local { from, to } => {
                drop(g);
                match to {
                    Some(target) => {
                        if msg.data.is_empty() {
                            target.deliver(msg.data, from);
                        }
                        for chunk in msg.data.chunks(segment) {
                            target.deliver(chunk, from);
                        }
                    }
                    // Each segment is a datagram of its own, and refused on its own.
                    None if msg.data.is_empty() => self.port_unreachable(dst, msg.data),
                    None => {
                        for chunk in msg.data.chunks(segment) {
                            self.port_unreachable(dst, chunk);
                        }
                    }
                }
                Ok(msg.data.len())
            }
            // Lost on a link where nothing answers.
            UdpRoute::Nowhere => Ok(msg.data.len()),
            UdpRoute::Backend { host_dst } => {
                let b = self.backend_endpoint(&mut g)?;
                drop(g);
                if msg.data.len() <= segment {
                    return b.send_to(msg.data, host_dst);
                }
                // One GSO send is one skb on Linux: it fails whole or goes out whole.
                for (i, chunk) in msg.data.chunks(segment).enumerate() {
                    match b.send_to(chunk, host_dst) {
                        Ok(_) => {}
                        Err(e) if i == 0 => return Err(e),
                        // The rest is lost on the way, as segments may be.
                        Err(_) => break,
                    }
                }
                Ok(msg.data.len())
            }
        }
    }

    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> NetResult<RecvMsg> {
        if flags.contains(MsgFlags::MSG_ERRQUEUE) {
            return self.recv_error(buf);
        }
        let peek = flags.0 & MsgFlags::MSG_PEEK.0 != 0;
        let mut g = self.lock();
        // `__skb_try_recv_datagram` reports a pending error before queued data.
        if let Some(e) = Self::take_err(&mut g) {
            return Err(e);
        }
        if let Some(d) = g.queue.front() {
            // Copied out under the lock, so a peek takes no copy of the datagram.
            let (full, from) = (d.data.len(), d.from);
            let len = buf.len().min(full);
            buf[..len].copy_from_slice(&d.data[..len]);
            if !peek {
                g.queue.pop_front();
                g.queued -= full + DGRAM_OVERHEAD;
            }
            return Ok(dgram_result(self.domain, len, full, from));
        }
        let backend = g.backend.clone();
        let rd_shut = g.rd_shut;
        drop(g);
        if let Some(b) = backend {
            match b.recv_from(buf, peek) {
                Ok((len, full_len, from)) => {
                    let from = self.net.guest_view(from);
                    return Ok(dgram_result(self.domain, len, full_len, from));
                }
                Err(Errno::EAGAIN) => {}
                Err(e) => return Err(e),
            }
        }
        if rd_shut {
            return Ok(RecvMsg::default());
        }
        Err(Errno::EAGAIN)
    }

    fn local_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        Ok(g.local
            .or(g.saddr.map(|ip| SocketAddr::new(ip, 0)))
            .map_or(unbound(self.domain), |a| present(self.domain, a)))
    }

    fn peer_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        g.peer
            .map(|p| present(self.domain, p))
            .ok_or(Errno::ENOTCONN)
    }

    fn take_error(&self) -> Option<Errno> {
        Self::take_err(&mut self.lock())
    }

    fn input_len(&self) -> NetResult<usize> {
        let g = self.lock();
        if let Some(d) = g.queue.front() {
            return Ok(d.data.len());
        }
        Ok(g.backend.as_ref().map_or(0, |b| b.input_len()))
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        let mut g = self.lock();
        if crate::mcast::handles(level, name) {
            return g.opts.set_mcast(&self.net, level, name, value);
        }
        let bound = g.local.is_some();
        let changed = g.opts.set(level, name, value, bound)?;
        if RECVERR.contains(&(level, name)) {
            // Turning either off purges the whole queue (`skb_errqueue_purge`), leaving the
            // pending error as it is.
            if !g.opts.flag(level, name) {
                let purged: usize = g
                    .errqueue
                    .drain(..)
                    .map(|e| e.payload.len() + DGRAM_OVERHEAD)
                    .sum();
                g.queued -= purged;
            }
            if let Some(b) = &g.backend {
                let on = i32::from(g.opts.flag(level, name));
                Self::forward_option(&**b, level, name, &on.to_le_bytes());
            }
        }
        if changed == Changed::Reuse
            && let Some(local) = g.local
        {
            self.net.state().udp.set_reuse(
                self.id,
                local.port(),
                g.opts.reuseaddr,
                g.opts.reuseport,
            );
        }
        Ok(())
    }

    fn graft(&self, ino: u64) {
        self.ino.store(ino, Ordering::Relaxed);
    }

    fn get_option(&self, level: i32, name: i32, out: &mut [u8]) -> NetResult<usize> {
        let mut g = self.lock();
        if level == SocketLevel::SOL_SOCKET.0 && name == SocketOption::SO_ERROR.0 {
            let e = Self::take_err(&mut g).map_or(0, |e| e.0);
            return Ok(crate::opts::write_int(out, e));
        }
        if level == SocketLevel::SOL_SOCKET.0 && name == SocketOption::SO_ACCEPTCONN.0 {
            return Ok(crate::opts::write_int(out, 0));
        }
        g.opts.get(level, name, out)
    }
}

/// What `ip_recv_error` / `ipv6_recv_error` return for `e`: the quoted payload, cut to `buf`
/// with `MSG_TRUNC` but its copied length returned even with `MSG_TRUNC` asked for, the
/// quoted datagram's destination as the sender, and the extended error with its offender in
/// the socket family's control message.
fn error_result(domain: Domain, e: &QueuedError, buf: &mut [u8]) -> RecvMsg {
    let len = buf.len().min(e.payload.len());
    buf[..len].copy_from_slice(&e.payload[..len]);
    let mut flags = MsgFlags::MSG_ERRQUEUE;
    if len < e.payload.len() {
        flags |= MsgFlags::MSG_TRUNC;
    }
    let ee = SockExtendedErr {
        ee_errno: e.errno.0 as u32,
        ee_origin: e.origin.0,
        ee_type: e.type_,
        ee_code: e.code,
        ee_pad: 0,
        ee_info: e.info,
        ee_data: e.data,
    };
    let mut data = vec![0; SockExtendedErr::SIZE];
    ee.write_to(&mut data);
    data.extend(sockaddr_bytes(domain, e.offender));
    let (level, kind) = match domain {
        Domain::Inet => RECVERR[0],
        Domain::Inet6 => RECVERR[1],
    };
    RecvMsg {
        len,
        full_len: len,
        from: e.dst.map(|d| present(domain, canonical(d))),
        flags,
        control: vec![Control { level, kind, data }],
    }
}

fn dgram_result(domain: Domain, len: usize, full_len: usize, from: SocketAddr) -> RecvMsg {
    RecvMsg {
        len,
        full_len,
        from: Some(present(domain, from)),
        flags: if full_len > len {
            MsgFlags::MSG_TRUNC
        } else {
            MsgFlags(0)
        },
        control: Vec::new(),
    }
}
