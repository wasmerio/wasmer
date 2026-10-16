//! TCP sockets: binding, listening with accept and SYN queues, connecting inside the namespace
//! over an in-memory [`Conn`] or outside it over a backend stream.

use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use wasixv2_abi::{Errno, MsgFlags, SocketLevel, SocketOption, TcpOption};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Accepted, BackendStream, Domain, NetResult, PortEntry, Publication, RecvMsg, SendMsg, Shutdown,
    Socket, SocketKind, StreamState,
    addr::{canonical, connect_target, present, unbound},
    net::{Net, Path},
    opts::{self, Changed, SockOpts},
    ports::{BindSpec, SockId},
    stream::{Buffers, Conn},
};

/// What carries a connected socket's bytes.
#[derive(Clone)]
enum Transport {
    /// In-namespace connection; the `usize` is this socket's side.
    Local(Arc<Conn>, usize),
    /// Through the backend.
    Remote(Arc<dyn BackendStream>),
}

enum Phase {
    Pending,
    Established,
    Failed,
}

impl Transport {
    fn phase(&self) -> Phase {
        match self {
            Self::Local(c, side) => {
                if c.is_established() {
                    Phase::Established
                } else if c.is_done(*side) {
                    Phase::Failed
                } else {
                    Phase::Pending
                }
            }
            Self::Remote(s) => match s.state() {
                StreamState::Connecting => Phase::Pending,
                StreamState::Established => Phase::Established,
                StreamState::Closed => Phase::Failed,
            },
        }
    }

    /// The transport's readiness. Both kinds wake the socket's own queue, which the socket
    /// registers on.
    fn poll(&self) -> PollMask {
        match self {
            Self::Local(c, side) => c.poll(*side),
            Self::Remote(s) => s.poll(None),
        }
    }

    fn read(&self, buf: &mut [u8], peek: bool) -> NetResult<usize> {
        match self {
            Self::Local(c, side) => c.read(*side, buf, peek),
            Self::Remote(s) => s.read(buf, peek),
        }
    }

    fn write(&self, data: &[u8]) -> NetResult<usize> {
        match self {
            Self::Local(c, side) => c.write(*side, data),
            Self::Remote(s) => s.write(data),
        }
    }

    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        match self {
            Self::Local(c, side) => {
                c.shutdown(*side, how);
                Ok(())
            }
            Self::Remote(s) => s.shutdown(how),
        }
    }

    /// `tcp_close` (or `tcp_disconnect` with `abort`). Dropping a backend stream closes it.
    fn close(self, abort: bool) {
        match self {
            Self::Local(c, side) => c.close(side, abort),
            Self::Remote(s) => {
                if abort {
                    let mut linger = [0u8; 8];
                    linger[..4].copy_from_slice(&1i32.to_le_bytes());
                    let _ = s.set_option(
                        SocketLevel::SOL_SOCKET.0,
                        SocketOption::SO_LINGER.0,
                        &linger,
                    );
                }
            }
        }
    }

    fn take_error(&self) -> Option<Errno> {
        match self {
            Self::Local(c, side) => c.take_error(*side),
            Self::Remote(s) => s.take_error(),
        }
    }

    fn input_len(&self) -> usize {
        match self {
            Self::Local(c, side) => c.input_len(*side),
            Self::Remote(s) => s.input_len(),
        }
    }

    /// `tcp_done` happened: reset, refused or aborted.
    fn is_done(&self) -> bool {
        match self {
            Self::Local(c, side) => c.is_done(*side),
            Self::Remote(s) => s.state() == StreamState::Closed,
        }
    }
}

enum State {
    Closed,
    Listening(Arc<Listener>),
    Connecting(Transport, SocketAddr),
    Connected(Transport, SocketAddr),
}

struct Inner {
    opts: SockOpts,
    /// Canonical bound address, once bound.
    local: Option<SocketAddr>,
    /// `SOCK_BINDPORT_LOCK`: the port was chosen by `bind`, so it survives a disconnect.
    port_locked: bool,
    state: State,
    /// `sk_err` while not connected.
    err: Option<Errno>,
    /// A connect failed asynchronously; the next `connect` reports it.
    failed: bool,
    rd_shut: bool,
    wr_shut: bool,
}

/// A TCP socket of a [`Net`].
pub struct TcpSocket {
    net: Arc<Net>,
    id: SockId,
    domain: Domain,
    wq: WaitQueue,
    me: Weak<TcpSocket>,
    /// The listener while listening; a leaf lock so lookups under the network lock can reach
    /// it without taking the socket lock.
    listener: Mutex<Option<Arc<Listener>>>,
    inner: Mutex<Inner>,
}

/// A listening socket's queues (`inet_connection_sock`'s accept queue and SYN queue).
pub(crate) struct Listener {
    wq: WaitQueue,
    inner: Mutex<ListenerInner>,
    publication: Mutex<Option<Box<dyn Publication>>>,
}

struct ListenerInner {
    backlog: u32,
    closed: bool,
    /// Established connections waiting for `accept`.
    ready: VecDeque<(Arc<TcpSocket>, SocketAddr)>,
    /// Connections that arrived while the accept queue was full; the clients stay in
    /// `SYN_SENT` until `accept` makes room, standing in for SYN retransmission.
    syn: VecDeque<(Arc<TcpSocket>, Arc<Conn>, SocketAddr)>,
}

impl ListenerInner {
    /// `sk_acceptq_is_full`: one more than the backlog fits.
    fn full(&self) -> bool {
        self.ready.len() > self.backlog as usize
    }
}

fn bufs(o: &SockOpts) -> Buffers {
    Buffers {
        sndbuf: o.sndbuf,
        rcvbuf: o.rcvbuf,
        rcvlowat: o.rcvlowat,
    }
}

impl TcpSocket {
    pub(crate) fn new(net: Arc<Net>, domain: Domain) -> Arc<Self> {
        Self::with_state(
            net,
            domain,
            SockOpts::new(SocketKind::Stream, domain),
            None,
            State::Closed,
            WaitQueue::new(),
        )
    }

    /// A socket whose one wait queue is `wq`: everything that changes its readiness, its own
    /// transport included, wakes that queue.
    fn with_state(
        net: Arc<Net>,
        domain: Domain,
        opts: SockOpts,
        local: Option<SocketAddr>,
        state: State,
        wq: WaitQueue,
    ) -> Arc<Self> {
        let id = net.next_id();
        Arc::new_cyclic(|me| Self {
            net,
            id,
            domain,
            wq,
            me: me.clone(),
            listener: Mutex::new(None),
            inner: Mutex::new(Inner {
                opts,
                local,
                port_locked: local.is_some(),
                state,
                err: None,
                failed: false,
                rd_shut: false,
                wr_shut: false,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        let mut g = self.inner.lock().unwrap();
        self.refresh(&mut g);
        g
    }

    fn spec(&self, o: &SockOpts, ip: std::net::IpAddr) -> BindSpec {
        BindSpec {
            family: self.domain,
            addr: ip,
            v6only: o.v6only,
            reuseaddr: o.reuseaddr,
            reuseport: o.reuseport,
            uid: 0,
        }
    }

    /// Completes a connection attempt the far side has decided.
    fn refresh(&self, g: &mut Inner) {
        let State::Connecting(t, peer) = &g.state else {
            return;
        };
        match t.phase() {
            Phase::Pending => {}
            Phase::Established => {
                g.state = State::Connected(t.clone(), *peer);
            }
            Phase::Failed => {
                // `tcp_done`: state CLOSE, both directions shut, autobound port released.
                g.err = t.take_error().or(Some(Errno::ECONNREFUSED));
                g.failed = true;
                g.rd_shut = true;
                g.wr_shut = true;
                g.state = State::Closed;
                self.put_port(g);
            }
        }
    }

    /// `inet_put_port` unless the user bound the port.
    fn put_port(&self, g: &mut Inner) {
        if !g.port_locked
            && let Some(local) = g.local.take()
        {
            self.net.state().tcp.unbind(self.id, local.port());
        }
    }

    fn autobind(&self, g: &mut Inner) -> NetResult<()> {
        if g.local.is_none() {
            let ip = self.domain_any();
            let port = self
                .net
                .tcp_bind(self.id, self.spec(&g.opts, ip), 0, self.me.clone())?;
            g.local = Some(SocketAddr::new(ip, port));
        }
        Ok(())
    }

    fn domain_any(&self) -> std::net::IpAddr {
        match self.domain {
            Domain::Inet => std::net::Ipv4Addr::UNSPECIFIED.into(),
            Domain::Inet6 => std::net::Ipv6Addr::UNSPECIFIED.into(),
        }
    }

    pub(crate) fn port_entry(&self) -> Option<PortEntry> {
        let g = self.inner.lock().unwrap();
        let local = present(self.domain, g.local?);
        let (peer, listening) = match &g.state {
            State::Connected(_, p) | State::Connecting(_, p) => {
                (Some(present(self.domain, *p)), false)
            }
            State::Listening(_) => (None, true),
            State::Closed => (None, false),
        };
        Some(PortEntry {
            kind: SocketKind::Stream,
            local,
            peer,
            listening,
        })
    }

    /// Stops listening (`inet_csk_listen_stop`): established children are reset, clients
    /// still in the SYN queue are refused.
    fn stop_listening(&self, g: &mut Inner) {
        if !matches!(g.state, State::Listening(_)) {
            return;
        }
        let State::Listening(l) = std::mem::replace(&mut g.state, State::Closed) else {
            unreachable!("checked above");
        };
        *self.listener.lock().unwrap() = None;
        if let Some(local) = g.local {
            self.net
                .state()
                .tcp
                .set_listening(self.id, local.port(), false);
        }
        l.publication.lock().unwrap().take();
        let (ready, syn) = {
            let mut li = l.inner.lock().unwrap();
            li.closed = true;
            (std::mem::take(&mut li.ready), std::mem::take(&mut li.syn))
        };
        for (child, _) in ready {
            child.release_inner(true);
        }
        for (child, conn, _) in syn {
            conn.refuse();
            child.release_inner(true);
        }
        self.wq.wake(PollMask::empty());
    }

    fn release_inner(&self, abort: bool) {
        let mut g = self.inner.lock().unwrap();
        self.stop_listening(&mut g);
        let abort = abort || g.opts.linger == Some(0);
        match std::mem::replace(&mut g.state, State::Closed) {
            State::Connecting(t, _) | State::Connected(t, _) => t.close(abort),
            _ => {}
        }
        if let Some(local) = g.local.take() {
            self.net.state().tcp.unbind(self.id, local.port());
        }
    }

    /// A child for a connection to the listener `self`, sharing its port, with `wq` as its
    /// queue.
    fn new_child(
        &self,
        local: SocketAddr,
        peer: SocketAddr,
        wq: WaitQueue,
        transport: impl FnOnce(&WaitQueue, &SockOpts) -> Transport,
    ) -> Arc<TcpSocket> {
        let opts = self.inner.lock().unwrap().opts.inherit();
        let child = Self::with_state(
            self.net.clone(),
            self.domain,
            opts.clone(),
            Some(local),
            State::Closed,
            wq,
        );
        let t = transport(&child.wq, &opts);
        child.inner.lock().unwrap().state = State::Connected(t, peer);
        self.net.state().tcp.inherit(
            child.id,
            self.spec(&opts, local.ip()),
            local.port(),
            peer,
            Arc::downgrade(&child),
        );
        child
    }

    /// A connection accepted by the backend for a published port; `wq` is the queue the stream
    /// wakes.
    pub(crate) fn incoming_remote(
        &self,
        stream: Arc<dyn BackendStream>,
        wq: WaitQueue,
        local: SocketAddr,
        peer: SocketAddr,
    ) -> bool {
        let Some(l) = self.listener.lock().unwrap().clone() else {
            return false;
        };
        {
            let li = l.inner.lock().unwrap();
            if li.closed || li.full() {
                return false;
            }
        }
        let child = self.new_child(local, peer, wq, |_, _| Transport::Remote(stream));
        let mut li = l.inner.lock().unwrap();
        if li.closed || li.full() {
            drop(li);
            child.release_inner(true);
            return false;
        }
        li.ready.push_back((child, peer));
        drop(li);
        l.wq.wake(PollMask::IN | PollMask::RDNORM);
        true
    }

    /// Connects to a listener of this namespace.
    fn connect_local(&self, g: &mut Inner, dst: SocketAddr) -> NetResult<()> {
        let local = g.local.expect("bound before connecting");
        let Some(listening) = self.net.find_listener(dst, local) else {
            self.put_port(g);
            return Err(Errno::ECONNREFUSED);
        };
        let Some(l) = listening.listener.lock().unwrap().clone() else {
            self.put_port(g);
            return Err(Errno::ECONNREFUSED);
        };
        let client_bufs = bufs(&g.opts);
        let mut conn = None;
        let child = listening.new_child(dst, local, WaitQueue::new(), |child_wq, child_opts| {
            let c = Arc::new(Conn::new(
                [self.wq.clone(), child_wq.clone()],
                [client_bufs, bufs(child_opts)],
                false,
            ));
            conn = Some(c.clone());
            Transport::Local(c, 1)
        });
        let conn = conn.expect("transport built");
        let mut li = l.inner.lock().unwrap();
        if li.closed {
            drop(li);
            child.release_inner(true);
            self.put_port(g);
            return Err(Errno::ECONNREFUSED);
        }
        self.net
            .state()
            .tcp
            .set_remote(self.id, local.port(), Some(dst));
        let t = Transport::Local(conn.clone(), 0);
        if li.full() {
            li.syn.push_back((child, conn, local));
            g.state = State::Connecting(t, dst);
            return Err(Errno::EINPROGRESS);
        }
        conn.establish();
        li.ready.push_back((child, local));
        drop(li);
        l.wq.wake(PollMask::IN | PollMask::RDNORM);
        g.state = State::Connected(t, dst);
        Ok(())
    }
}

impl Listener {
    /// Takes the next established connection and moves SYN-queue entries up while room.
    fn pop(&self) -> Option<(Arc<TcpSocket>, SocketAddr)> {
        let mut li = self.inner.lock().unwrap();
        let next = li.ready.pop_front()?;
        let mut drop_list = Vec::new();
        while !li.full() {
            let Some((child, conn, peer)) = li.syn.pop_front() else {
                break;
            };
            if conn.is_closed(0) {
                drop_list.push(child);
                continue;
            }
            conn.establish();
            li.ready.push_back((child, peer));
        }
        drop(li);
        for child in drop_list {
            child.release_inner(true);
        }
        Some(next)
    }
}

impl Pollable for TcpSocket {
    /// `tcp_poll` and `inet_csk_listen_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        // The one queue, whatever the state: the transport wakes it too.
        poll_wait(&mut table, &self.wq);
        let g = self.lock();
        match &g.state {
            State::Listening(l) => {
                if l.inner.lock().unwrap().ready.is_empty() {
                    PollMask::empty()
                } else {
                    PollMask::IN | PollMask::RDNORM
                }
            }
            State::Connecting(t, _) | State::Connected(t, _) => {
                let t = t.clone();
                drop(g);
                t.poll()
            }
            State::Closed => {
                let mut m = PollMask::HUP | PollMask::OUT | PollMask::WRNORM;
                if g.rd_shut {
                    m |= PollMask::IN | PollMask::RDNORM | PollMask::RDHUP;
                }
                if g.err.is_some() {
                    m |= PollMask::ERR;
                }
                m
            }
        }
    }
}

impl Socket for TcpSocket {
    fn domain(&self) -> Domain {
        self.domain
    }

    fn kind(&self) -> SocketKind {
        SocketKind::Stream
    }

    fn protocol(&self) -> i32 {
        6
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
        if g.local.is_some() || !matches!(g.state, State::Closed) {
            return Err(Errno::EINVAL);
        }
        let port = self.net.tcp_bind(
            self.id,
            self.spec(&g.opts, addr.ip()),
            addr.port(),
            self.me.clone(),
        )?;
        g.local = Some(SocketAddr::new(addr.ip(), port));
        g.port_locked = addr.port() != 0;
        Ok(())
    }

    fn listen(&self, backlog: u32) -> NetResult<()> {
        let mut g = self.lock();
        match &g.state {
            State::Listening(l) => {
                l.inner.lock().unwrap().backlog = backlog;
                return Ok(());
            }
            State::Connecting(..) | State::Connected(..) => return Err(Errno::EINVAL),
            State::Closed => {}
        }
        self.autobind(&mut g)?;
        let local = g.local.expect("bound");
        self.net.state().tcp.listen(self.id, local.port())?;
        let l = Arc::new(Listener {
            wq: self.wq.clone(),
            inner: Mutex::new(ListenerInner {
                backlog,
                closed: false,
                ready: VecDeque::new(),
                syn: VecDeque::new(),
            }),
            publication: Mutex::new(None),
        });
        *self.listener.lock().unwrap() = Some(l.clone());
        g.state = State::Listening(l.clone());
        g.rd_shut = false;
        g.wr_shut = false;
        drop(g);
        self.auto_publish(&l, local);
        Ok(())
    }

    fn accept(&self) -> NetResult<Accepted> {
        let l = self.listener.lock().unwrap().clone().ok_or(Errno::EINVAL)?;
        let (socket, peer) = l.pop().ok_or(Errno::EAGAIN)?;
        Ok(Accepted {
            socket,
            peer: present(self.domain, peer),
        })
    }

    fn connect(&self, addr: SocketAddr) -> NetResult<()> {
        let mapped = matches!(addr, SocketAddr::V6(v6) if v6.ip().to_ipv4_mapped().is_some());
        let dst = connect_target(canonical(addr));
        let mut g = self.lock();
        match &g.state {
            State::Listening(_) | State::Connected(..) => return Err(Errno::EISCONN),
            State::Connecting(..) => return Err(Errno::EALREADY),
            State::Closed if g.failed => {
                // `__inet_stream_connect` after an asynchronous failure: report it once.
                g.failed = false;
                return Err(g.err.take().unwrap_or(Errno::ECONNABORTED));
            }
            State::Closed => {}
        }
        if mapped && g.opts.v6only {
            return Err(Errno::ENETUNREACH);
        }
        let path = self.net.path(dst, g.local.map(|l| l.ip()))?;
        let src = match &path {
            Path::Local { src } | Path::Backend { src, .. } => *src,
        };
        match g.local {
            None => {
                let port = self.net.tcp_bind_connect(
                    self.id,
                    self.spec(&g.opts, src),
                    dst,
                    self.me.clone(),
                )?;
                g.local = Some(SocketAddr::new(src, port));
            }
            Some(local) if local.ip().is_unspecified() => {
                self.net.state().tcp.rehash(self.id, local.port(), src);
                g.local = Some(SocketAddr::new(src, local.port()));
            }
            Some(_) => {}
        }
        g.rd_shut = false;
        g.wr_shut = false;
        g.err = None;
        match path {
            Path::Local { .. } => self.connect_local(&mut g, dst),
            Path::Backend { host_dst, .. } => {
                let backend = self.net.backend.clone().ok_or(Errno::ENETUNREACH)?;
                match backend.connect(host_dst, self.wq.clone()) {
                    Ok(stream) => {
                        g.state = State::Connecting(Transport::Remote(stream), dst);
                        Err(Errno::EINPROGRESS)
                    }
                    Err(e) => {
                        self.put_port(&mut g);
                        Err(e)
                    }
                }
            }
        }
    }

    fn disconnect(&self) -> NetResult<()> {
        // `tcp_disconnect`: abort whatever is going on and return to CLOSE.
        let mut g = self.lock();
        self.stop_listening(&mut g);
        if let State::Connecting(t, _) | State::Connected(t, _) =
            std::mem::replace(&mut g.state, State::Closed)
        {
            t.close(true);
        }
        g.rd_shut = false;
        g.wr_shut = false;
        g.failed = false;
        self.put_port(&mut g);
        drop(g);
        self.wq.wake(PollMask::empty());
        Ok(())
    }

    fn shutdown(&self, how: Shutdown) -> NetResult<()> {
        let mut g = self.lock();
        match &g.state {
            State::Listening(_) => {
                if how != Shutdown::Write {
                    self.stop_listening(&mut g);
                }
                Ok(())
            }
            State::Connecting(..) => {
                if let State::Connecting(t, _) = std::mem::replace(&mut g.state, State::Closed) {
                    t.close(true);
                }
                self.put_port(&mut g);
                drop(g);
                self.wq.wake(PollMask::empty());
                Ok(())
            }
            State::Connected(t, _) => {
                let t = t.clone();
                drop(g);
                t.shutdown(how)
            }
            State::Closed => {
                // `inet_shutdown` on a CLOSE socket: record it, wake pollers, ENOTCONN.
                g.rd_shut |= how != Shutdown::Write;
                g.wr_shut |= how != Shutdown::Read;
                drop(g);
                self.wq.wake(PollMask::empty());
                Err(Errno::ENOTCONN)
            }
        }
    }

    fn send(&self, msg: SendMsg<'_>) -> NetResult<usize> {
        if msg.flags.0 & MsgFlags::MSG_OOB.0 != 0 {
            // TODO: urgent data is not supported.
            return Err(Errno::EOPNOTSUPP);
        }
        // Linux's TCP ignores a destination address on a connected socket.
        let mut g = self.lock();
        let t = match &g.state {
            State::Connected(t, _) => t.clone(),
            State::Connecting(..) => return Err(Errno::EAGAIN),
            State::Listening(_) | State::Closed => {
                return Err(g.err.take().unwrap_or(Errno::EPIPE));
            }
        };
        drop(g);
        t.write(msg.data)
    }

    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> NetResult<RecvMsg> {
        if flags.0 & MsgFlags::MSG_OOB.0 != 0 {
            return Err(Errno::EINVAL);
        }
        if flags.0 & MsgFlags::MSG_ERRQUEUE.0 != 0 {
            return Err(Errno::EAGAIN);
        }
        let peek = flags.0 & MsgFlags::MSG_PEEK.0 != 0;
        let mut g = self.lock();
        let t = match &g.state {
            State::Connected(t, _) | State::Connecting(t, _) => t.clone(),
            State::Listening(_) => return Err(Errno::ENOTCONN),
            State::Closed => {
                if let Some(e) = g.err.take() {
                    return Err(e);
                }
                if g.rd_shut {
                    return Ok(RecvMsg::default());
                }
                return Err(Errno::ENOTCONN);
            }
        };
        drop(g);
        // MSG_TRUNC on TCP discards the bytes instead of copying them.
        let len = if flags.0 & MsgFlags::MSG_TRUNC.0 != 0 {
            let mut scratch = vec![0; buf.len()];
            t.read(&mut scratch, peek)?
        } else {
            t.read(buf, peek)?
        };
        Ok(RecvMsg {
            len,
            full_len: len,
            ..RecvMsg::default()
        })
    }

    fn local_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        Ok(g.local
            .map_or(unbound(self.domain), |a| present(self.domain, a)))
    }

    fn peer_addr(&self) -> NetResult<SocketAddr> {
        let g = self.lock();
        match &g.state {
            State::Connected(t, peer) if !t.is_done() => Ok(present(self.domain, *peer)),
            _ => Err(Errno::ENOTCONN),
        }
    }

    fn take_error(&self) -> Option<Errno> {
        let mut g = self.lock();
        match &g.state {
            State::Connected(t, _) | State::Connecting(t, _) => t.take_error().or(g.err.take()),
            _ => g.err.take(),
        }
    }

    fn input_len(&self) -> NetResult<usize> {
        let g = self.lock();
        match &g.state {
            State::Listening(_) => Err(Errno::EINVAL),
            State::Connected(t, _) | State::Connecting(t, _) => Ok(t.input_len()),
            State::Closed => Ok(0),
        }
    }

    fn output_len(&self) -> NetResult<usize> {
        let g = self.lock();
        match &g.state {
            State::Listening(_) => Err(Errno::EINVAL),
            State::Connected(Transport::Local(c, side), _) => Ok(c.output_len(*side)),
            _ => Ok(0),
        }
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> NetResult<()> {
        let mut g = self.lock();
        let bound = g.local.is_some();
        let changed = g.opts.set(level, name, value, bound)?;
        match changed {
            Changed::Reuse => {
                if let Some(local) = g.local {
                    self.net.state().tcp.set_reuse(
                        self.id,
                        local.port(),
                        g.opts.reuseaddr,
                        g.opts.reuseport,
                    );
                }
            }
            Changed::Buffers => {
                if let State::Connected(Transport::Local(c, side), _) = &g.state {
                    c.set_buffers(*side, bufs(&g.opts));
                }
            }
            Changed::Nothing => {}
        }
        if let State::Connected(Transport::Remote(s), _)
        | State::Connecting(Transport::Remote(s), _) = &g.state
        {
            s.set_option(level, name, value)?;
        }
        Ok(())
    }

    fn get_option(&self, level: i32, name: i32, out: &mut [u8]) -> NetResult<usize> {
        let mut g = self.lock();
        if level == SocketLevel::SOL_SOCKET.0 && name == SocketOption::SO_ACCEPTCONN.0 {
            return Ok(opts::write_int(
                out,
                matches!(g.state, State::Listening(_)) as i32,
            ));
        }
        if level == SocketLevel::SOL_SOCKET.0 && name == SocketOption::SO_ERROR.0 {
            let err = match &g.state {
                State::Connected(t, _) | State::Connecting(t, _) => t.take_error(),
                _ => g.err.take(),
            };
            return Ok(opts::write_int(out, err.map_or(0, |e| e.0)));
        }
        if level == SocketLevel::SOL_TCP.0 && name == TcpOption::TCP_MAXSEG.0 {
            let mss = match &g.state {
                State::Connected(Transport::Local(..), _) => opts::TCP_MSS_LOOPBACK,
                State::Connected(..) => 1460,
                _ if g.opts.user_mss != 0 => g.opts.user_mss,
                _ => opts::TCP_MSS_DEFAULT,
            };
            return Ok(opts::write_int(out, mss));
        }
        if level == SocketLevel::SOL_TCP.0 && name == TcpOption::TCP_INFO.0 {
            // Synthesized: only `tcpi_state` is meaningful (DESIGN.md "Not supported").
            let state: u8 = match &g.state {
                State::Connected(t, _) if t.is_done() => 7,
                State::Connected(..) => 1,
                State::Connecting(..) => 2,
                State::Listening(_) => 10,
                State::Closed => 7,
            };
            let n = out.len().min(TCP_INFO_LEN);
            out[..n].fill(0);
            if n > 0 {
                out[0] = state;
            }
            return Ok(n);
        }
        g.opts.get(level, name, out)
    }
}

/// `sizeof(struct tcp_info)` in current Linux.
const TCP_INFO_LEN: usize = 248;

impl TcpSocket {
    /// Auto-publishing: mirror a non-loopback listener on the host. Failure is logged only.
    fn auto_publish(&self, l: &Listener, local: SocketAddr) {
        let (Some(host_ip), Some(backend)) =
            (self.net.config.auto_publish, self.net.backend.as_ref())
        else {
            return;
        };
        if local.ip().is_loopback() {
            return;
        }
        let host = SocketAddr::new(host_ip, local.port());
        match backend.publish(host, self.net.inbound_sink(local)) {
            Ok(p) => *l.publication.lock().unwrap() = Some(p),
            Err(e) => tracing::warn!(%host, guest = %local, error = %e, "auto-publish failed"),
        }
    }
}

impl Drop for TcpSocket {
    /// The kernel dropped its last reference: `close`.
    fn drop(&mut self) {
        self.release_inner(false);
    }
}
