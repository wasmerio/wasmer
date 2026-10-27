//! UDP sockets: datagrams move directly between the receive queues of a namespace's sockets;
//! datagrams for other destinations go through a backend datagram endpoint.

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use wasixv2_abi::{Errno, MsgFlags, SocketLevel, SocketOption};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, BackendDatagram, Domain, NetResult, PortEntry, RecvMsg, SendMsg, Shutdown, Socket,
    SocketKind,
    addr::{canonical, present, unbound},
    cmsg,
    net::{Net, Path, UdpRoute},
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

struct Datagram {
    data: Vec<u8>,
    /// Canonical sender.
    from: SocketAddr,
}

struct Inner {
    opts: SockOpts,
    local: Option<SocketAddr>,
    /// `SOCK_BINDADDR_LOCK` / `SOCK_BINDPORT_LOCK`.
    addr_locked: bool,
    port_locked: bool,
    peer: Option<SocketAddr>,
    queue: VecDeque<Datagram>,
    /// `sk_rmem_alloc`.
    queued: usize,
    err: Option<Errno>,
    rd_shut: bool,
    wr_shut: bool,
    backend: Option<Arc<dyn BackendDatagram>>,
}

/// A UDP socket of a [`Net`].
pub struct UdpSocket {
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    /// `sk_uid`, for `SO_REUSEPORT`.
    uid: u32,
    wq: WaitQueue,
    me: Weak<UdpSocket>,
    inner: Mutex<Inner>,
}

impl UdpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain, uid: u32) -> Arc<Self> {
        let id = net.next_id();
        Arc::new_cyclic(|me| Self {
            net,
            id,
            domain,
            uid,
            wq: WaitQueue::new(),
            me: me.clone(),
            inner: Mutex::new(Inner {
                opts: SockOpts::new(SocketKind::Datagram, domain),
                local: None,
                addr_locked: false,
                port_locked: false,
                peer: None,
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                rd_shut: false,
                wr_shut: false,
                backend: None,
            }),
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
        let ip = self.any();
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
        let backend = self.net.backend.as_ref().ok_or(Errno::ENETUNREACH)?;
        let b = backend.datagram(self.domain, self.wq.clone())?;
        g.backend = Some(b.clone());
        Ok(b)
    }

    /// `sock_error`: the socket's pending error, else the backend endpoint's.
    fn take_err(g: &mut Inner) -> Option<Errno> {
        g.err
            .take()
            .or_else(|| g.backend.as_ref().and_then(|b| b.take_error()))
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        let local = self.inner.get_mut().unwrap().local.take();
        if let Some(local) = local {
            self.net.state().udp.unbind(self.id, local.port());
        }
    }
}

impl Pollable for UdpSocket {
    /// `udp_poll` / `datagram_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wq);
        let g = self.lock();
        let mut m = PollMask::empty();
        if g.err.is_some() {
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
        let port = self.net.udp_bind(
            self.id,
            self.spec(&g.opts, addr.ip()),
            addr.port(),
            self.me.clone(),
        )?;
        g.local = Some(SocketAddr::new(addr.ip(), port));
        g.addr_locked = !addr.ip().is_unspecified();
        g.port_locked = addr.port() != 0;
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
        let path = self.net.path(dst, g.local.map(|l| l.ip()))?;
        let src = match path {
            Path::Local { src } | Path::Backend { src, .. } => src,
        };
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
            // The association moved inside the namespace: nothing from outside is for us now.
            Path::Local { .. } => g.backend = None,
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
        if let Some(local) = g.local {
            let mut st = self.net.state();
            st.udp.set_remote(self.id, local.port(), None);
            if !g.port_locked {
                st.udp.unbind(self.id, local.port());
                g.local = None;
            } else if !g.addr_locked {
                st.udp.rehash(self.id, local.port(), self.any());
                g.local = Some(SocketAddr::new(self.any(), local.port()));
            }
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
            && (v4.is_broadcast() || dst.ip().is_multicast())
        {
            // Broadcast and multicast are not carried (TODO); Linux still checks SO_BROADCAST.
            if v4.is_broadcast()
                && !g
                    .opts
                    .flag(SocketLevel::SOL_SOCKET.0, SocketOption::SO_BROADCAST.0)
            {
                return Err(Errno::EACCES);
            }
            return Ok(msg.data.len());
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
        let connected = g.peer.is_some();
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
                    None if connected => {
                        // ICMP port unreachable reaches connected sockets as ECONNREFUSED.
                        self.lock().err = Some(Errno::ECONNREFUSED);
                        self.wq.wake(PollMask::ERR);
                    }
                    None => {}
                }
                Ok(msg.data.len())
            }
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
        if flags.0 & MsgFlags::MSG_ERRQUEUE.0 != 0 {
            return Err(Errno::EAGAIN);
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
        let bound = g.local.is_some();
        let changed = g.opts.set(level, name, value, bound)?;
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
