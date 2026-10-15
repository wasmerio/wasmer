//! The mio reactor thread and the guest-facing stream, datagram and publication objects.

use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use std::os::fd::AsRawFd;

use mio::{
    Events, Interest, Poll, Registry, Token, Waker,
    net::{TcpListener, TcpStream, UdpSocket},
    unix::SourceFd,
};
use wasixv2_abi::{Errno, SocketLevel, SocketOption, TcpOption};
use wasixv2_vnet::{
    Backend, BackendDatagram, BackendStream, Domain, InboundSink, Publication, Shutdown,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::errno_from_io;

const WAKER: Token = Token(0);
/// Bytes buffered per direction of a stream.
const STREAM_BUF: usize = 256 * 1024;
/// Bytes of datagrams buffered for the guest.
const DGRAM_BUF: usize = 256 * 1024;
const READ_CHUNK: usize = 64 * 1024;

enum Entry {
    Stream(Arc<StreamShared>),
    Listener(Arc<ListenerShared>),
    Datagram(Arc<DgramShared>),
}

struct Shared {
    registry: Registry,
    waker: Waker,
    entries: Mutex<HashMap<Token, Entry>>,
    /// Streams the guest side touched; the reactor pumps them on the next wake.
    dirty: Mutex<Vec<Token>>,
    next: AtomicUsize,
    stop: AtomicBool,
}

impl Shared {
    fn token(&self) -> Token {
        Token(self.next.fetch_add(1, Ordering::Relaxed))
    }

    fn kick(&self, token: Token) {
        self.dirty.lock().unwrap().push(token);
        let _ = self.waker.wake();
    }

    fn entry(&self, token: Token) -> Option<Entry> {
        self.entries.lock().unwrap().get(&token).map(|e| match e {
            Entry::Stream(s) => Entry::Stream(s.clone()),
            Entry::Listener(l) => Entry::Listener(l.clone()),
            Entry::Datagram(d) => Entry::Datagram(d.clone()),
        })
    }

    fn remove(&self, token: Token) {
        self.entries.lock().unwrap().remove(&token);
    }
}

/// A [`Backend`] over host sockets, driven by its own mio reactor thread.
pub struct HostBackend {
    shared: Arc<Shared>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HostBackend {
    pub fn new() -> io::Result<Arc<Self>> {
        let poll = Poll::new()?;
        let registry = poll.registry().try_clone()?;
        let waker = Waker::new(poll.registry(), WAKER)?;
        let shared = Arc::new(Shared {
            registry,
            waker,
            entries: Mutex::new(HashMap::new()),
            dirty: Mutex::new(Vec::new()),
            next: AtomicUsize::new(1),
            stop: AtomicBool::new(false),
        });
        let thread = thread::Builder::new()
            .name("wasixv2-vnet-host".into())
            .spawn({
                let shared = shared.clone();
                move || run(poll, shared)
            })?;
        Ok(Arc::new(Self {
            shared,
            thread: Some(thread),
        }))
    }
}

impl Drop for HostBackend {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.shared.waker.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(mut poll: Poll, shared: Arc<Shared>) {
    let mut events = Events::with_capacity(256);
    let mut buf = vec![0u8; READ_CHUNK];
    while !shared.stop.load(Ordering::Acquire) {
        if let Err(e) = poll.poll(&mut events, None) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            tracing::error!(error = %e, "vnet-host reactor failed");
            return;
        }
        for ev in events.iter() {
            if ev.token() == WAKER {
                continue;
            }
            match shared.entry(ev.token()) {
                Some(Entry::Stream(s)) => {
                    {
                        let mut st = s.state();
                        st.host_readable |=
                            ev.is_readable() || ev.is_read_closed() || ev.is_error();
                        st.host_writable |= ev.is_writable() || ev.is_error();
                    }
                    s.pump(&shared, &mut buf);
                }
                Some(Entry::Listener(l)) => l.accept_all(),
                Some(Entry::Datagram(d)) => d.receive_all(&mut buf),
                None => {}
            }
        }
        let dirty = std::mem::take(&mut *shared.dirty.lock().unwrap());
        for token in dirty {
            if let Some(Entry::Stream(s)) = shared.entry(token) {
                s.pump(&shared, &mut buf);
            }
        }
    }
}

// ---- streams ----

#[derive(Default)]
struct StreamState {
    connected: bool,
    /// The connection attempt failed (`ERR`/`HUP` until the socket goes).
    failed: bool,
    err: Option<Errno>,
    /// Host to guest.
    rx: VecDeque<u8>,
    rx_eof: bool,
    /// Guest to host.
    tx: VecDeque<u8>,
    /// Guest `shutdown(Write)`; the host socket is shut once `tx` drains.
    wr_shut: bool,
    host_wr_shut: bool,
    rd_shut: bool,
    /// The connection was reset or failed on the host.
    reset: bool,
    host_readable: bool,
    host_writable: bool,
    /// The guest dropped its handle.
    released: bool,
    /// Close with RST (`SO_LINGER` zero, or unread data at close).
    abort: bool,
}

struct StreamShared {
    sock: TcpStream,
    token: Token,
    peer: SocketAddr,
    wq: WaitQueue,
    st: Mutex<StreamState>,
    backend: Weak<Shared>,
}

impl StreamShared {
    fn state(&self) -> MutexGuard<'_, StreamState> {
        self.st.lock().unwrap()
    }

    fn kick(&self) {
        if let Some(b) = self.backend.upgrade() {
            b.kick(self.token);
        }
    }

    /// Moves bytes between the host socket and the buffers. Runs on the reactor thread.
    fn pump(&self, shared: &Shared, buf: &mut [u8]) {
        let mut st = self.state();
        let mut wake = PollMask::empty();
        if !st.connected && !st.failed {
            if !st.host_writable && !st.host_readable {
                return;
            }
            match self.sock.take_error() {
                Ok(Some(e)) | Err(e) => {
                    st.failed = true;
                    st.err = Some(errno_from_io(&e));
                    wake |= PollMask::ERR | PollMask::HUP;
                }
                Ok(None) => match self.sock.peer_addr() {
                    Ok(_) => {
                        st.connected = true;
                        wake |= PollMask::OUT | PollMask::WRNORM;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotConnected => {
                        st.host_writable = false;
                        return;
                    }
                    Err(e) => {
                        st.failed = true;
                        st.err = Some(errno_from_io(&e));
                        wake |= PollMask::ERR | PollMask::HUP;
                    }
                },
            }
        }
        if st.connected && !st.reset {
            while st.host_readable && !st.rx_eof && st.rx.len() < STREAM_BUF {
                let want = (STREAM_BUF - st.rx.len()).min(buf.len());
                match (&self.sock).read(&mut buf[..want]) {
                    Ok(0) => {
                        st.rx_eof = true;
                        wake |= PollMask::IN | PollMask::RDHUP;
                    }
                    Ok(n) => {
                        st.rx.extend(&buf[..n]);
                        wake |= PollMask::IN | PollMask::RDNORM;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => st.host_readable = false,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        st.reset = true;
                        st.err = Some(errno_from_io(&e));
                        wake |= PollMask::ERR | PollMask::HUP;
                    }
                }
            }
            while st.host_writable && !st.tx.is_empty() && !st.reset {
                let (chunk, _) = st.tx.as_slices();
                match (&self.sock).write(chunk) {
                    Ok(n) => {
                        st.tx.drain(..n);
                        wake |= PollMask::OUT | PollMask::WRNORM;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => st.host_writable = false,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        st.reset = true;
                        st.err = Some(errno_from_io(&e));
                        wake |= PollMask::ERR | PollMask::HUP;
                    }
                }
            }
            if st.wr_shut && st.tx.is_empty() && !st.host_wr_shut {
                st.host_wr_shut = true;
                let _ = self.sock.shutdown(std::net::Shutdown::Write);
            }
        }
        let finished = st.released && (st.abort || st.tx.is_empty() || st.reset || st.failed);
        if finished {
            if st.abort {
                let _ = socket2::SockRef::from(&self.sock).set_linger(Some(Duration::ZERO));
            } else {
                let _ = self.sock.shutdown(std::net::Shutdown::Both);
            }
            drop(st);
            self.deregister(shared);
            return;
        }
        drop(st);
        if !wake.is_empty() {
            self.wq.wake(wake);
        }
    }

    fn deregister(&self, shared: &Shared) {
        let fd = self.sock.as_raw_fd();
        let _ = shared.registry.deregister(&mut SourceFd(&fd));
        shared.remove(self.token);
    }
}

/// The guest's handle on a host stream; dropping it closes the connection.
struct HostStream(Arc<StreamShared>);

impl HostStream {
    fn register(
        backend: &Arc<Shared>,
        sock: TcpStream,
        peer: SocketAddr,
        connected: bool,
    ) -> io::Result<Arc<Self>> {
        let token = backend.token();
        let shared = Arc::new(StreamShared {
            sock,
            token,
            peer,
            wq: WaitQueue::new(),
            st: Mutex::new(StreamState {
                connected,
                ..StreamState::default()
            }),
            backend: Arc::downgrade(backend),
        });
        backend
            .entries
            .lock()
            .unwrap()
            .insert(token, Entry::Stream(shared.clone()));
        let fd = shared.sock.as_raw_fd();
        if let Err(e) = backend.registry.register(
            &mut SourceFd(&fd),
            token,
            Interest::READABLE | Interest::WRITABLE,
        ) {
            backend.remove(token);
            return Err(e);
        }
        Ok(Arc::new(Self(shared)))
    }
}

impl Drop for HostStream {
    fn drop(&mut self) {
        let mut st = self.0.state();
        st.released = true;
        st.abort |= !st.rx.is_empty();
        drop(st);
        self.0.kick();
    }
}

impl Pollable for HostStream {
    /// What `tcp_poll` reports for the guest socket this stream backs.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wq);
        let st = self.0.state();
        let mut m = PollMask::empty();
        if st.err.is_some() {
            m |= PollMask::ERR;
        }
        if st.failed {
            return m
                | PollMask::HUP
                | PollMask::IN
                | PollMask::RDNORM
                | PollMask::RDHUP
                | PollMask::OUT
                | PollMask::WRNORM;
        }
        if !st.connected {
            return m;
        }
        let rcv_shut = st.rx_eof || st.rd_shut || st.reset;
        let snd_shut = st.wr_shut || st.reset;
        if rcv_shut && snd_shut {
            m |= PollMask::HUP;
        }
        if rcv_shut {
            m |= PollMask::IN | PollMask::RDNORM | PollMask::RDHUP;
        }
        if !st.rx.is_empty() {
            m |= PollMask::IN | PollMask::RDNORM;
        }
        if snd_shut || st.tx.len() < STREAM_BUF {
            m |= PollMask::OUT | PollMask::WRNORM;
        }
        m
    }
}

impl BackendStream for HostStream {
    fn read(&self, buf: &mut [u8], peek: bool) -> Result<usize, Errno> {
        let mut st = self.0.state();
        if !st.rx.is_empty() {
            let n = buf.len().min(st.rx.len());
            for (dst, src) in buf[..n].iter_mut().zip(st.rx.iter()) {
                *dst = *src;
            }
            if !peek {
                let was_full = st.rx.len() >= STREAM_BUF;
                st.rx.drain(..n);
                drop(st);
                if was_full {
                    self.0.kick();
                }
            }
            return Ok(n);
        }
        if st.rx_eof {
            return Ok(0);
        }
        if let Some(e) = st.err.take() {
            return Err(e);
        }
        if st.rd_shut || st.reset || st.failed {
            return Ok(0);
        }
        Err(Errno::EAGAIN)
    }

    fn write(&self, data: &[u8]) -> Result<usize, Errno> {
        let mut st = self.0.state();
        if st.err.is_some() || st.wr_shut || st.reset || st.failed {
            return Err(st.err.take().unwrap_or(Errno::EPIPE));
        }
        if !st.connected {
            return Err(Errno::EAGAIN);
        }
        let space = STREAM_BUF.saturating_sub(st.tx.len());
        if space == 0 {
            return Err(Errno::EAGAIN);
        }
        let n = space.min(data.len());
        st.tx.extend(&data[..n]);
        drop(st);
        self.0.kick();
        Ok(n)
    }

    fn shutdown(&self, how: Shutdown) -> Result<(), Errno> {
        let mut st = self.0.state();
        if how != Shutdown::Write {
            st.rd_shut = true;
        }
        if how != Shutdown::Read {
            st.wr_shut = true;
        }
        drop(st);
        self.0.kick();
        self.0.wq.wake(PollMask::empty());
        Ok(())
    }

    fn take_error(&self) -> Option<Errno> {
        self.0.state().err.take()
    }

    fn peer_addr(&self) -> SocketAddr {
        self.0.peer
    }

    fn input_len(&self) -> usize {
        self.0.state().rx.len()
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        let int = || {
            value
                .get(..4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        };
        let sock = socket2::SockRef::from(&self.0.sock);
        let (Some(v), sol_socket) = (int(), level == SocketLevel::SOL_SOCKET.0) else {
            return Ok(());
        };
        if sol_socket && name == SocketOption::SO_LINGER.0 {
            let secs = value
                .get(4..8)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()));
            self.0.state().abort = v != 0 && secs == Some(0);
        } else if sol_socket && name == SocketOption::SO_KEEPALIVE.0 {
            let _ = sock.set_keepalive(v != 0);
        } else if level == SocketLevel::SOL_TCP.0 && name == TcpOption::TCP_NODELAY.0 {
            let _ = sock.set_tcp_nodelay(v != 0);
        }
        Ok(())
    }
}

// ---- publishing ----

struct ListenerShared {
    sock: TcpListener,
    token: Token,
    sink: Arc<dyn InboundSink>,
    backend: Weak<Shared>,
}

impl ListenerShared {
    fn accept_all(&self) {
        let Some(backend) = self.backend.upgrade() else {
            return;
        };
        loop {
            match self.sock.accept() {
                Ok((sock, peer)) => match HostStream::register(&backend, sock, peer, true) {
                    Ok(stream) => {
                        if !self.sink.incoming(stream) {
                            tracing::debug!(%peer, "published connection refused by the guest");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "registering published connection"),
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    tracing::warn!(error = %e, "accept on published port");
                    return;
                }
            }
        }
    }
}

struct HostPublication {
    shared: Arc<ListenerShared>,
    backend: Weak<Shared>,
    addr: SocketAddr,
}

impl Publication for HostPublication {
    fn host_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for HostPublication {
    fn drop(&mut self) {
        if let Some(b) = self.backend.upgrade() {
            let fd = self.shared.sock.as_raw_fd();
            let _ = b.registry.deregister(&mut SourceFd(&fd));
            b.remove(self.shared.token);
        }
    }
}

// ---- datagrams ----

#[derive(Default)]
struct DgramState {
    queue: VecDeque<(Vec<u8>, SocketAddr)>,
    queued: usize,
}

struct DgramShared {
    sock: UdpSocket,
    token: Token,
    wq: WaitQueue,
    st: Mutex<DgramState>,
    backend: Weak<Shared>,
}

impl DgramShared {
    fn receive_all(&self, buf: &mut [u8]) {
        let mut got = false;
        loop {
            match self.sock.recv_from(buf) {
                Ok((n, from)) => {
                    let mut st = self.st.lock().unwrap();
                    // Like a full SO_RCVBUF: drop when the buffer is over its limit.
                    if st.queued <= DGRAM_BUF {
                        st.queued += n;
                        st.queue.push_back((buf[..n].to_vec(), from));
                        got = true;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // ICMP errors on unconnected sockets are not reported (no IP_RECVERR).
                Err(_) => {}
            }
        }
        if got {
            self.wq.wake(PollMask::IN | PollMask::RDNORM);
        }
    }
}

struct HostDatagram(Arc<DgramShared>);

impl Drop for HostDatagram {
    fn drop(&mut self) {
        if let Some(b) = self.0.backend.upgrade() {
            let fd = self.0.sock.as_raw_fd();
            let _ = b.registry.deregister(&mut SourceFd(&fd));
            b.remove(self.0.token);
        }
    }
}

impl Pollable for HostDatagram {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wq);
        let mut m = PollMask::OUT | PollMask::WRNORM;
        if !self.0.st.lock().unwrap().queue.is_empty() {
            m |= PollMask::IN | PollMask::RDNORM;
        }
        m
    }
}

impl BackendDatagram for HostDatagram {
    fn send_to(&self, data: &[u8], to: SocketAddr) -> Result<usize, Errno> {
        self.0.sock.send_to(data, to).map_err(|e| errno_from_io(&e))
    }

    fn recv_from(&self, buf: &mut [u8], peek: bool) -> Result<(usize, usize, SocketAddr), Errno> {
        let mut st = self.0.st.lock().unwrap();
        let (data, from) = st.queue.front().ok_or(Errno::EAGAIN)?;
        let (from, full) = (*from, data.len());
        let n = buf.len().min(full);
        buf[..n].copy_from_slice(&data[..n]);
        if !peek {
            st.queue.pop_front();
            st.queued -= full;
        }
        Ok((n, full, from))
    }

    fn input_len(&self) -> usize {
        self.0
            .st
            .lock()
            .unwrap()
            .queue
            .front()
            .map_or(0, |d| d.0.len())
    }
}

// ---- the backend ----

impl Backend for HostBackend {
    fn connect(&self, peer: SocketAddr) -> Result<Arc<dyn BackendStream>, Errno> {
        let sock = TcpStream::connect(peer).map_err(|e| errno_from_io(&e))?;
        let stream =
            HostStream::register(&self.shared, sock, peer, false).map_err(|e| errno_from_io(&e))?;
        Ok(stream)
    }

    fn datagram(&self, domain: Domain) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let any: SocketAddr = match domain {
            Domain::Inet => (Ipv4Addr::UNSPECIFIED, 0).into(),
            Domain::Inet6 => (Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let sock = UdpSocket::bind(any).map_err(|e| errno_from_io(&e))?;
        let token = self.shared.token();
        let shared = Arc::new(DgramShared {
            sock,
            token,
            wq: WaitQueue::new(),
            st: Mutex::new(DgramState::default()),
            backend: Arc::downgrade(&self.shared),
        });
        self.shared
            .entries
            .lock()
            .unwrap()
            .insert(token, Entry::Datagram(shared.clone()));
        let fd = shared.sock.as_raw_fd();
        self.shared
            .registry
            .register(&mut SourceFd(&fd), token, Interest::READABLE)
            .map_err(|e| errno_from_io(&e))?;
        Ok(Arc::new(HostDatagram(shared)))
    }

    fn publish(
        &self,
        host: SocketAddr,
        sink: Arc<dyn InboundSink>,
    ) -> io::Result<Box<dyn Publication>> {
        let sock = TcpListener::bind(host)?;
        let addr = sock.local_addr()?;
        let token = self.shared.token();
        let shared = Arc::new(ListenerShared {
            sock,
            token,
            sink,
            backend: Arc::downgrade(&self.shared),
        });
        self.shared
            .entries
            .lock()
            .unwrap()
            .insert(token, Entry::Listener(shared.clone()));
        let fd = shared.sock.as_raw_fd();
        if let Err(e) = self
            .shared
            .registry
            .register(&mut SourceFd(&fd), token, Interest::READABLE)
        {
            self.shared.remove(token);
            return Err(e);
        }
        Ok(Box::new(HostPublication {
            shared,
            backend: Arc::downgrade(&self.shared),
            addr,
        }))
    }
}
