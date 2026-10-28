//! The mio reactor thread and the guest-facing stream, datagram and publication objects.
//!
//! Every object wakes the wait queue of the guest socket it serves, handed over when the object
//! is made, so a waiter registered on the socket before the object existed is not lost.

use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use std::os::fd::AsRawFd;

use mio::{
    Events, Interest, Poll, Registry, Token, Waker,
    net::{TcpListener, TcpStream, UdpSocket},
    unix::SourceFd,
};
use wasixv2_abi::{Errno, IpOption, Ipv6Option, SocketLevel, SocketOption, TcpOption};
use wasixv2_vnet::{
    Backend, BackendDatagram, BackendStream, Domain, InboundSink, Publication, QueuedError, Room,
    Shutdown, StreamState as Phase,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitEntry, WaitQueue, WaitRegistration};

use crate::errno_from_io;

const WAKER: Token = Token(0);
/// Bytes buffered per direction of a stream.
const STREAM_BUF: usize = 256 * 1024;
/// Bytes of datagrams buffered for the guest.
const DGRAM_BUF: usize = 256 * 1024;
const READ_CHUNK: usize = 64 * 1024;
/// How much room a guest read must open in a full receive buffer before the reactor reads
/// from the host again, as `__tcp_cleanup_rbuf` reopens the window only once it can grow
/// substantially rather than after every small read.
const RX_REOPEN: usize = READ_CHUNK;
/// How long a stream the guest closed may go without the host peer taking any of its unsent
/// bytes before it is reset, standing in for `tcp_fin_timeout` and the orphan retransmission
/// limit (`tcp_orphan_retries`) that bound an orphaned Linux socket.
const ORPHAN_TIMEOUT: Duration = Duration::from_secs(60);

enum Entry {
    Stream(Arc<StreamShared>),
    Listener(Arc<ListenerShared>),
    Datagram(Arc<DgramShared>),
}

struct Shared {
    registry: Registry,
    waker: Waker,
    entries: Mutex<HashMap<Token, Entry>>,
    /// Streams the guest side touched since the reactor last pumped them; each is listed at
    /// most once, guarded by its `dirty` flag.
    dirty: Mutex<Vec<Arc<StreamShared>>>,
    /// Publications whose guest listener made room since the reactor last accepted for them;
    /// each is listed at most once, guarded by its `pending` flag.
    listeners: Mutex<Vec<Arc<ListenerShared>>>,
    next: AtomicUsize,
    stop: AtomicBool,
}

impl Shared {
    fn token(&self) -> Token {
        Token(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// Marks `stream` dirty, waking the reactor only when it was not dirty already, so a burst
    /// of guest writes costs one wakeup and one pump.
    fn kick(&self, stream: &Arc<StreamShared>) {
        if stream.dirty.swap(true, Ordering::AcqRel) {
            return;
        }
        self.dirty.lock().unwrap().push(stream.clone());
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
        let removed = self.entries.lock().unwrap().remove(&token);
        drop(removed);
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
            listeners: Mutex::new(Vec::new()),
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
    // Streams the guest closed with bytes still unsent, by token.
    let mut orphans: HashMap<Token, Arc<StreamShared>> = HashMap::new();
    let mut timeout = None;
    while !shared.stop.load(Ordering::Acquire) {
        if let Err(e) = poll.poll(&mut events, timeout) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            tracing::error!(error = %e, "vnet-host reactor failed");
            return;
        }
        let mut pump = |s: &Arc<StreamShared>, buf: &mut [u8]| {
            if s.pump(&shared, buf) {
                orphans.insert(s.token, s.clone());
            }
        };
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
                    pump(&s, &mut buf);
                }
                Some(Entry::Listener(l)) => l.accept_all(),
                Some(Entry::Datagram(d)) => {
                    if ev.is_writable() || ev.is_error() {
                        d.writable();
                    }
                    if ev.is_readable() || ev.is_error() {
                        d.receive_all(&mut buf);
                    }
                }
                None => {}
            }
        }
        let dirty = std::mem::take(&mut *shared.dirty.lock().unwrap());
        for s in dirty {
            // Cleared before pumping: a guest write during the pump marks it again.
            s.dirty.store(false, Ordering::Release);
            pump(&s, &mut buf);
        }
        let listeners = std::mem::take(&mut *shared.listeners.lock().unwrap());
        for l in listeners {
            l.pending.store(false, Ordering::Release);
            l.accept_all();
        }
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        orphans.retain(|_, s| match s.expire(&shared, now) {
            Some(deadline) => {
                next = Some(next.map_or(deadline, |n| n.min(deadline)));
                true
            }
            None => false,
        });
        timeout = next.map(|deadline| deadline.saturating_duration_since(now));
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
    /// `SO_LINGER` with a timeout: the longest the unsent bytes may take after close.
    linger: Option<Duration>,
    /// While orphaned with bytes unsent: when the stream is reset if the host peer has taken
    /// nothing more by then.
    orphan_deadline: Option<Instant>,
    /// When the reactor first saw the stream released with bytes unsent.
    closed_at: Option<Instant>,
    /// Deregistered: the reactor is finished with it.
    gone: bool,
    keepalive: Keepalive,
}

/// The guest's keepalive settings, which the host socket gets while keepalive is on.
#[derive(Default)]
struct Keepalive {
    on: bool,
    idle: Option<Duration>,
    interval: Option<Duration>,
    count: Option<u32>,
}

impl Keepalive {
    fn apply(&self, sock: &socket2::SockRef<'_>) -> io::Result<()> {
        if !self.on {
            return sock.set_keepalive(false);
        }
        let mut params = socket2::TcpKeepalive::new();
        if let Some(idle) = self.idle {
            params = params.with_time(idle);
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
        ))]
        {
            if let Some(interval) = self.interval {
                params = params.with_interval(interval);
            }
            if let Some(count) = self.count {
                params = params.with_retries(count);
            }
        }
        // Turns keepalive on, then sets what the guest chose.
        sock.set_tcp_keepalive(&params)
    }
}

struct StreamShared {
    sock: TcpStream,
    token: Token,
    peer: SocketAddr,
    /// The guest socket's queue.
    wq: WaitQueue,
    /// Listed on `Shared::dirty`.
    dirty: AtomicBool,
    st: Mutex<StreamState>,
    backend: Weak<Shared>,
}

impl StreamShared {
    fn state(&self) -> MutexGuard<'_, StreamState> {
        self.st.lock().unwrap()
    }

    /// Moves bytes between the host socket and the buffers. Runs on the reactor thread.
    /// Returns whether the stream is now an orphan draining its last bytes.
    fn pump(self: &Arc<Self>, shared: &Shared, buf: &mut [u8]) -> bool {
        let mut st = self.state();
        if st.gone {
            return false;
        }
        if st.released && !st.connected {
            // `tcp_close` in `SYN_SENT` (or after a failed attempt): nothing to deliver.
            drop(st);
            self.close(shared);
            return false;
        }
        let mut wake = PollMask::empty();
        if !st.connected && !st.failed {
            if !st.host_writable && !st.host_readable {
                return false;
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
                        return false;
                    }
                    Err(e) => {
                        st.failed = true;
                        st.err = Some(errno_from_io(&e));
                        wake |= PollMask::ERR | PollMask::HUP;
                    }
                },
            }
        }
        let mut progress = false;
        if st.connected && !st.reset {
            // An orphan reads nothing more: what arrives after close is the host's to answer.
            while st.host_readable && !st.released && !st.rx_eof && st.rx.len() < STREAM_BUF {
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
                        progress = true;
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
        if st.released {
            let finished = st.abort || st.tx.is_empty() || st.reset || st.failed;
            if finished {
                drop(st);
                self.close(shared);
                return false;
            }
            // `tcp_close` with data still queued: the orphan drains while its peer makes
            // progress, and is reset once it stalls (or its `SO_LINGER` time is up).
            let now = Instant::now();
            let closed = *st.closed_at.get_or_insert(now);
            let idle = match st.orphan_deadline {
                Some(deadline) if !progress => deadline,
                _ => now + ORPHAN_TIMEOUT,
            };
            st.orphan_deadline = Some(match st.linger {
                Some(linger) => idle.min(closed + linger),
                None => idle,
            });
            return true;
        }
        drop(st);
        if !wake.is_empty() {
            self.wq.wake(wake);
        }
        false
    }

    /// For an orphan: resets it once its deadline passed. Returns the deadline while it is
    /// still draining, `None` once it is gone.
    fn expire(&self, shared: &Shared, now: Instant) -> Option<Instant> {
        let mut st = self.state();
        if st.gone {
            return None;
        }
        let deadline = st.orphan_deadline?;
        if deadline > now {
            return Some(deadline);
        }
        st.abort = true;
        drop(st);
        self.close(shared);
        None
    }

    /// The end of the host socket: RST when aborting, else FIN after everything queued, which
    /// the host kernel delivers on its own once the descriptor is closed.
    fn close(&self, shared: &Shared) {
        let abort = {
            let mut st = self.state();
            st.gone = true;
            st.abort
        };
        if abort {
            let _ = socket2::SockRef::from(&self.sock).set_linger(Some(Duration::ZERO));
        } else {
            let _ = self.sock.shutdown(std::net::Shutdown::Write);
        }
        let fd = self.sock.as_raw_fd();
        let _ = shared.registry.deregister(&mut SourceFd(&fd));
        shared.remove(self.token);
    }

    fn kick(self: &Arc<Self>) {
        if let Some(b) = self.backend.upgrade() {
            b.kick(self);
        }
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
        wq: WaitQueue,
    ) -> io::Result<Arc<Self>> {
        let token = backend.token();
        let shared = Arc::new(StreamShared {
            sock,
            token,
            peer,
            wq,
            dirty: AtomicBool::new(false),
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
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
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
    fn state(&self) -> Phase {
        let st = self.0.state();
        if st.failed || st.reset {
            Phase::Closed
        } else if st.connected {
            Phase::Established
        } else {
            Phase::Connecting
        }
    }

    fn read(&self, buf: &mut [u8], peek: bool) -> Result<usize, Errno> {
        let mut st = self.0.state();
        if !st.rx.is_empty() {
            let n = buf.len().min(st.rx.len());
            let (a, b) = st.rx.as_slices();
            let first = n.min(a.len());
            buf[..first].copy_from_slice(&a[..first]);
            buf[first..n].copy_from_slice(&b[..n - first]);
            if !peek {
                let free_before = STREAM_BUF.saturating_sub(st.rx.len());
                st.rx.drain(..n);
                drop(st);
                if free_before < RX_REOPEN && free_before + n >= RX_REOPEN {
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

    /// What the guest wrote that the host socket has not taken yet; what the host socket
    /// holds unacknowledged is not counted.
    fn output_len(&self) -> usize {
        self.0.state().tx.len()
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        let int = |at: usize| {
            value
                .get(at..at + 4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        };
        let sock = socket2::SockRef::from(&self.0.sock);
        let (Some(v), sol_socket) = (int(0), level == SocketLevel::SOL_SOCKET.0) else {
            return Ok(());
        };
        if sol_socket && name == SocketOption::SO_LINGER.0 {
            let secs = int(4);
            let mut st = self.0.state();
            st.abort = v != 0 && secs == Some(0);
            st.linger = match (v, secs) {
                (0, _) | (_, None) => None,
                (_, Some(secs)) => Some(Duration::from_secs(secs.max(0) as u64)),
            };
        } else if sol_socket && name == SocketOption::SO_KEEPALIVE.0 {
            let mut st = self.0.state();
            st.keepalive.on = v != 0;
            let _ = st.keepalive.apply(&sock);
        } else if level == SocketLevel::SOL_TCP.0 && name == TcpOption::TCP_NODELAY.0 {
            let _ = sock.set_tcp_nodelay(v != 0);
        } else if level == SocketLevel::SOL_TCP.0 {
            // The guest validated the value; the host takes it when keepalive is on, as
            // Linux's `tcp_sock_set_keep*` only rearm a running keepalive timer.
            let mut st = self.0.state();
            let secs = Duration::from_secs(v.max(1) as u64);
            match TcpOption(name) {
                TcpOption::TCP_KEEPIDLE => st.keepalive.idle = Some(secs),
                TcpOption::TCP_KEEPINTVL => st.keepalive.interval = Some(secs),
                TcpOption::TCP_KEEPCNT => st.keepalive.count = Some(v.max(1) as u32),
                _ => return Ok(()),
            }
            if st.keepalive.on {
                let _ = st.keepalive.apply(&sock);
            }
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
    /// Listed on `Shared::listeners`.
    pending: AtomicBool,
    /// Registrations on the guest listeners' room queues while their accept queues are full.
    room: Mutex<Vec<WaitRegistration>>,
}

impl ListenerShared {
    /// Takes connections off the host listener while the guest has room for them. A full guest
    /// accept queue leaves them in the host's backlog, as Linux leaves a SYN unanswered when
    /// `sk_acceptq_is_full`, until the guest makes room and wakes [`RoomKick`]; with no guest
    /// listener at all they are reset, as a SYN to a closed port is.
    fn accept_all(self: &Arc<Self>) {
        let Some(backend) = self.backend.upgrade() else {
            return;
        };
        loop {
            let room = match self.sink.poll_room(None) {
                // On the room queues before looking again, so no wakeup is lost.
                Room::Full => self.poll_room_registered(),
                room => room,
            };
            if room == Room::Full {
                return;
            }
            match self.sock.accept() {
                Ok((sock, peer)) if room == Room::Gone => {
                    tracing::debug!(%peer, "published connection without a guest listener");
                    let _ = socket2::SockRef::from(&sock).set_linger(Some(Duration::ZERO));
                }
                Ok((sock, peer)) => {
                    // The queue of the guest socket-to-be, which adopts it.
                    let wq = WaitQueue::new();
                    match HostStream::register(&backend, sock, peer, true, wq.clone()) {
                        Ok(stream) => {
                            if !self.sink.incoming(stream, wq) {
                                tracing::debug!(%peer, "published connection refused by the guest");
                            }
                        }
                        Err(e) => tracing::warn!(error = %e, "registering published connection"),
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    tracing::warn!(error = %e, "accept on published port");
                    return;
                }
            }
        }
    }

    /// `poll_room` with a registration that brings the reactor back to this listener.
    fn poll_room_registered(self: &Arc<Self>) -> Room {
        let entry: Arc<dyn WaitEntry> = Arc::new(RoomKick(Arc::downgrade(self)));
        let mut regs = Vec::new();
        let mut proc = |q: &WaitQueue, _| regs.push(q.add(entry.clone()));
        let room = self
            .sink
            .poll_room(Some(&mut PollTable::new(PollMask::empty(), &mut proc)));
        // The old registrations go outside the lock, which their queues' wakes never take.
        let old = std::mem::replace(&mut *self.room.lock().unwrap(), regs);
        drop(old);
        room
    }
}

/// Woken, under a guest listener's room queue lock, when its accept queue may have room:
/// lists the publication for the reactor, which accepts for it.
struct RoomKick(Weak<ListenerShared>);

impl WaitEntry for RoomKick {
    fn wake(&self, _: PollMask) -> bool {
        let Some(listener) = self.0.upgrade() else {
            return false;
        };
        let Some(backend) = listener.backend.upgrade() else {
            return false;
        };
        if !listener.pending.swap(true, Ordering::AcqRel) {
            backend.listeners.lock().unwrap().push(listener);
            let _ = backend.waker.wake();
        }
        true
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

    fn set_backlog(&self, backlog: u32) {
        // `listen` again on a listening socket only changes its backlog.
        let backlog = backlog.min(i32::MAX as u32) as i32;
        if let Err(e) = socket2::SockRef::from(&self.shared.sock).listen(backlog) {
            tracing::warn!(error = %e, "changing the backlog of a published port");
        }
    }
}

impl Drop for HostPublication {
    fn drop(&mut self) {
        self.shared.room.lock().unwrap().clear();
        if let Some(b) = self.backend.upgrade() {
            let fd = self.shared.sock.as_raw_fd();
            let _ = b.registry.deregister(&mut SourceFd(&fd));
            b.remove(self.shared.token);
        }
    }
}

// ---- datagrams ----

/// The options that make the host queue ICMP errors, as the guest names them.
const RECVERR: [(i32, i32); 2] = [
    (SocketLevel::SOL_IP.0, IpOption::IP_RECVERR.0),
    (SocketLevel::SOL_IPV6.0, Ipv6Option::IPV6_RECVERR.0),
];

struct DgramState {
    queue: VecDeque<(Vec<u8>, SocketAddr)>,
    /// Bytes of datagrams and of queued errors' payloads.
    queued: usize,
    /// `sk_err` of the host socket: what an ICMP error to a connected peer left, or with
    /// `IP_RECVERR` to any peer.
    err: Option<Errno>,
    /// `IP_RECVERR` and `IPV6_RECVERR` as set on the host socket.
    recverr: [bool; 2],
    /// The host socket's error queue, as far as the reactor has read it.
    errqueue: VecDeque<QueuedError>,
    /// The host socket is connected (`connect`), so the host filters by its peer.
    connected: Option<SocketAddr>,
    /// The host socket takes datagrams: false from a send's `EAGAIN` until the reactor sees
    /// it writable again.
    host_writable: bool,
}

struct DgramShared {
    sock: UdpSocket,
    token: Token,
    /// The guest socket's queue.
    wq: WaitQueue,
    st: Mutex<DgramState>,
    backend: Weak<Shared>,
}

impl DgramShared {
    fn state(&self) -> MutexGuard<'_, DgramState> {
        self.st.lock().unwrap()
    }

    /// The host socket became writable: wake senders that met `EAGAIN` (`sock_wfree` waking
    /// `sk_write_space`).
    fn writable(&self) {
        let was = std::mem::replace(&mut self.state().host_writable, true);
        if !was {
            self.wq
                .wake(PollMask::OUT | PollMask::WRNORM | PollMask::WRBAND);
        }
    }

    fn receive_all(&self, buf: &mut [u8]) {
        let mut wake = self.receive_datagrams(buf);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if self.state().recverr.contains(&true) {
            wake |= self.receive_errors(buf);
        }
        if !wake.is_empty() {
            self.wq.wake(wake);
        }
    }

    /// Reads the host socket's error queue into the endpoint's, which holds as much as its
    /// receive buffer (`sock_queue_err_skb`).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn receive_errors(&self, buf: &mut [u8]) -> PollMask {
        let mut wake = PollMask::empty();
        loop {
            match errqueue::recv(&self.sock, buf) {
                Ok(Some(e)) => {
                    let mut st = self.state();
                    if st.queued <= DGRAM_BUF {
                        st.queued += e.payload.len();
                        st.errqueue.push_back(e);
                        wake |= PollMask::ERR;
                    }
                }
                Ok(None) => return wake,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    tracing::warn!(error = %e, "reading a host datagram socket's error queue");
                    return wake;
                }
            }
        }
    }

    fn receive_datagrams(&self, buf: &mut [u8]) -> PollMask {
        let mut wake = PollMask::empty();
        // An error is reported once and then cleared (`sock_error`); one that comes straight
        // back is not a peer's report, and retrying it would starve every other socket.
        let mut failed = false;
        loop {
            match self.sock.recv_from(buf) {
                Ok((n, from)) => {
                    failed = false;
                    let mut st = self.state();
                    // Like a full SO_RCVBUF: drop when the buffer is over its limit.
                    if st.queued <= DGRAM_BUF {
                        st.queued += n;
                        st.queue.push_back((buf[..n].to_vec(), from));
                        wake |= PollMask::IN | PollMask::RDNORM;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if failed => {
                    tracing::warn!(error = %e, "receiving on a host datagram socket");
                    break;
                }
                // An ICMP error (`__udp4_lib_err`): for a connected socket, or with
                // IP_RECVERR for any; without it an unconnected socket never hears of them.
                Err(e) => {
                    failed = true;
                    let mut st = self.state();
                    if st.connected.is_some() || st.recverr.contains(&true) {
                        st.err = Some(errno_from_io(&e));
                        wake |= PollMask::ERR;
                    }
                }
            }
        }
        wake
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
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        let st = self.0.state();
        let mut m = PollMask::empty();
        if st.host_writable {
            m |= PollMask::OUT | PollMask::WRNORM;
        }
        if !st.queue.is_empty() {
            m |= PollMask::IN | PollMask::RDNORM;
        }
        if st.err.is_some() || !st.errqueue.is_empty() {
            m |= PollMask::ERR;
        }
        m
    }
}

impl BackendDatagram for HostDatagram {
    fn send_to(&self, data: &[u8], to: SocketAddr) -> Result<usize, Errno> {
        // Under the state lock, so the reactor marks the socket writable only after an
        // `EAGAIN` here has marked it full: no wakeup falls between the two.
        let mut st = self.0.state();
        // `send` to the connected peer, so the host reports its errors on this socket.
        let sent = match st.connected {
            Some(peer) if peer == to => self.0.sock.send(data),
            _ => self.0.sock.send_to(data, to),
        };
        match sent {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                st.host_writable = false;
                Err(Errno::EAGAIN)
            }
            Err(e) => Err(errno_from_io(&e)),
        }
    }

    fn recv_from(&self, buf: &mut [u8], peek: bool) -> Result<(usize, usize, SocketAddr), Errno> {
        let mut st = self.0.state();
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
        self.0.state().queue.front().map_or(0, |d| d.0.len())
    }

    fn connect(&self, peer: Option<SocketAddr>) -> Result<(), Errno> {
        let sock = socket2::SockRef::from(&self.0.sock);
        match peer {
            Some(peer) => sock.connect(&peer.into()),
            None => {
                // SAFETY: a zeroed `sockaddr_storage` is a valid `AF_UNSPEC` address, which
                // dissolves the association (`udp_disconnect`).
                let unspec =
                    unsafe { socket2::SockAddr::new(socket2::SockAddrStorage::zeroed(), 16) };
                match sock.connect(&unspec) {
                    // BSDs dissolve it too, but report EAFNOSUPPORT.
                    Err(e) if e.raw_os_error() == Some(Errno::EAFNOSUPPORT.0) => Ok(()),
                    result => result,
                }
            }
        }
        .map_err(|e| errno_from_io(&e))?;
        let mut st = self.0.state();
        st.connected = peer;
        st.err = None;
        Ok(())
    }

    fn take_error(&self) -> Option<Errno> {
        self.0.state().err.take()
    }

    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        let Some(i) = RECVERR.iter().position(|&o| o == (level, name)) else {
            return Ok(());
        };
        let on = value.iter().any(|&b| b != 0);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        errqueue::set_recverr(&self.0.sock, i == 1, on).map_err(|e| errno_from_io(&e))?;
        let mut st = self.0.state();
        st.recverr[i] = on;
        if !on {
            // The host purged its queue (`skb_errqueue_purge`); so does the endpoint.
            let purged: usize = st.errqueue.drain(..).map(|e| e.payload.len()).sum();
            st.queued -= purged;
        }
        Ok(())
    }

    fn take_queued_error(&self) -> Option<QueuedError> {
        let mut st = self.0.state();
        let e = st.errqueue.pop_front()?;
        st.queued -= e.payload.len();
        // `sock_dequeue_err_skb`.
        match st.errqueue.front() {
            Some(next) if next.is_icmp() => st.err = Some(next.errno),
            _ if e.is_icmp() => st.err = None,
            _ => {}
        }
        Some(e)
    }
}

// ---- the backend ----

impl Backend for HostBackend {
    fn connect(&self, peer: SocketAddr, wq: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        let sock = TcpStream::connect(peer).map_err(|e| errno_from_io(&e))?;
        let stream = HostStream::register(&self.shared, sock, peer, false, wq)
            .map_err(|e| errno_from_io(&e))?;
        Ok(stream)
    }

    fn datagram(&self, domain: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let any: SocketAddr = match domain {
            Domain::Inet => (Ipv4Addr::UNSPECIFIED, 0).into(),
            Domain::Inet6 => (Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let sock = UdpSocket::bind(any).map_err(|e| errno_from_io(&e))?;
        let token = self.shared.token();
        let shared = Arc::new(DgramShared {
            sock,
            token,
            wq,
            st: Mutex::new(DgramState {
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                recverr: [false; 2],
                errqueue: VecDeque::new(),
                connected: None,
                host_writable: true,
            }),
            backend: Arc::downgrade(&self.shared),
        });
        self.shared
            .entries
            .lock()
            .unwrap()
            .insert(token, Entry::Datagram(shared.clone()));
        let fd = shared.sock.as_raw_fd();
        // Writable too: a send that met `EAGAIN` waits for the host to take datagrams again.
        if let Err(e) = self.shared.registry.register(
            &mut SourceFd(&fd),
            token,
            Interest::READABLE | Interest::WRITABLE,
        ) {
            self.shared.remove(token);
            return Err(errno_from_io(&e));
        }
        Ok(Arc::new(HostDatagram(shared)))
    }

    fn publish(
        &self,
        host: SocketAddr,
        backlog: u32,
        sink: Arc<dyn InboundSink>,
    ) -> io::Result<Box<dyn Publication>> {
        let sock = listen(host, backlog)?;
        let addr = sock.local_addr()?;
        let token = self.shared.token();
        let shared = Arc::new(ListenerShared {
            sock,
            token,
            sink,
            backend: Arc::downgrade(&self.shared),
            pending: AtomicBool::new(false),
            room: Mutex::new(Vec::new()),
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

/// A non-blocking host listener on `addr` with `backlog`, as `TcpListener::bind` makes one
/// (`SO_REUSEADDR` on Unix, so a published port is free again at once after a restart).
fn listen(addr: SocketAddr, backlog: u32) -> io::Result<TcpListener> {
    let sock = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    sock.listen(backlog.min(i32::MAX as u32) as i32)?;
    Ok(TcpListener::from_std(sock.into()))
}

/// The host's socket error queue (Linux's `IP_RECVERR`), which socket2 has no API for.
#[cfg(any(target_os = "linux", target_os = "android"))]
mod errqueue {
    use std::{io, mem, os::fd::AsRawFd, ptr};

    use mio::net::UdpSocket;
    use wasixv2_abi::{Errno, SoEeOrigin};
    use wasixv2_vnet::QueuedError;

    /// `IP_RECVERR`, or `IPV6_RECVERR` for `v6`, on the host socket.
    pub(super) fn set_recverr(sock: &UdpSocket, v6: bool, on: bool) -> io::Result<()> {
        let (level, name) = match v6 {
            false => (libc::SOL_IP, libc::IP_RECVERR),
            true => (libc::SOL_IPV6, libc::IPV6_RECVERR),
        };
        let value = libc::c_int::from(on);
        // SAFETY: an int option, passed with its size, on a descriptor the socket owns.
        let r = unsafe {
            libc::setsockopt(
                sock.as_raw_fd(),
                level,
                name,
                ptr::from_ref(&value).cast(),
                mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        match r {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Takes one entry off the host socket's error queue (`recvmsg(MSG_ERRQUEUE)`), its quoted
    /// payload read through `buf`; `None` once the queue is empty. Entries without an
    /// `IP_RECVERR`/`IPV6_RECVERR` message are skipped.
    pub(super) fn recv(sock: &UdpSocket, buf: &mut [u8]) -> io::Result<Option<QueuedError>> {
        loop {
            // Room for the largest message: the extended error and a sockaddr_in6.
            let mut control = [0u64; 16];
            let mut iov = libc::iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: buf.len(),
            };
            // SAFETY: `recvmsg` writes at most `len` bytes of address into the storage and
            // the lengths it reports back; every buffer outlives the call.
            let received = unsafe {
                socket2::SockAddr::try_init(|storage, len| {
                    let mut msg: libc::msghdr = mem::zeroed();
                    msg.msg_name = storage.cast();
                    msg.msg_namelen = *len;
                    msg.msg_iov = &mut iov;
                    msg.msg_iovlen = 1;
                    msg.msg_control = control.as_mut_ptr().cast();
                    msg.msg_controllen = mem::size_of_val(&control) as _;
                    let flags = libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT;
                    let n = libc::recvmsg(sock.as_raw_fd(), &mut msg, flags);
                    if n < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    *len = msg.msg_namelen;
                    Ok((n as usize, msg))
                })
            };
            let ((n, msg), dst) = match received {
                Ok(r) => r,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
            };
            let payload = buf[..n.min(buf.len())].to_vec();
            // SAFETY: `msg` describes `control`, which `recvmsg` filled.
            if let Some(e) = unsafe { extended_err(&msg) } {
                return Ok(Some(QueuedError {
                    dst: dst.as_socket(),
                    payload,
                    ..e
                }));
            }
        }
    }

    /// The `IP_RECVERR`/`IPV6_RECVERR` message among `msg`'s control messages, without the
    /// address and payload.
    ///
    /// # Safety
    ///
    /// `msg` must describe control messages `recvmsg` filled in.
    unsafe fn extended_err(msg: &libc::msghdr) -> Option<QueuedError> {
        const EE_LEN: usize = mem::size_of::<libc::sock_extended_err>();
        // SAFETY: the caller's; the walk stays inside `msg_controllen`.
        unsafe {
            let mut cmsg = libc::CMSG_FIRSTHDR(msg);
            while !cmsg.is_null() {
                let h = &*cmsg;
                let ours = (h.cmsg_level == libc::SOL_IP && h.cmsg_type == libc::IP_RECVERR)
                    || (h.cmsg_level == libc::SOL_IPV6 && h.cmsg_type == libc::IPV6_RECVERR);
                let data = libc::CMSG_DATA(cmsg);
                let len =
                    (h.cmsg_len as usize).saturating_sub(data.offset_from(cmsg.cast()) as usize);
                if ours && len >= EE_LEN {
                    let ee: libc::sock_extended_err = ptr::read_unaligned(data.cast());
                    return Some(QueuedError {
                        errno: Errno(ee.ee_errno as i32),
                        origin: SoEeOrigin(ee.ee_origin),
                        type_: ee.ee_type,
                        code: ee.ee_code,
                        info: ee.ee_info,
                        data: ee.ee_data,
                        offender: sockaddr(data.add(EE_LEN), len - EE_LEN),
                        dst: None,
                        payload: Vec::new(),
                    });
                }
                cmsg = libc::CMSG_NXTHDR(msg, cmsg);
            }
        }
        None
    }

    /// The socket address of `len` bytes at `raw`; `None` for `AF_UNSPEC` (no offender).
    ///
    /// # Safety
    ///
    /// `raw` must be readable for `len` bytes.
    unsafe fn sockaddr(raw: *const u8, len: usize) -> Option<std::net::SocketAddr> {
        let len = len.min(mem::size_of::<libc::sockaddr_storage>());
        // SAFETY: at most the storage's size is copied, from the caller's readable bytes.
        let (_, addr) = unsafe {
            socket2::SockAddr::try_init(|storage, l| {
                ptr::copy_nonoverlapping(raw, storage.cast::<u8>(), len);
                *l = len as libc::socklen_t;
                Ok(())
            })
        }
        .ok()?;
        addr.as_socket()
    }
}

#[cfg(test)]
mod tests {
    use std::{os::fd::FromRawFd, sync::mpsc};

    use super::*;

    fn backend() -> Arc<HostBackend> {
        HostBackend::new().unwrap()
    }

    #[test]
    fn keepalive_parameters_reach_the_host_socket_once_keepalive_is_on() {
        let b = backend();
        let host = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let sock = TcpStream::connect(host.local_addr().unwrap()).unwrap();
        let peer = sock.peer_addr().unwrap_or(host.local_addr().unwrap());
        let s = HostStream::register(&b.shared, sock, peer, false, WaitQueue::new()).unwrap();
        let int = |v: i32| v.to_le_bytes();
        let tcp = SocketLevel::SOL_TCP.0;
        s.set_option(tcp, TcpOption::TCP_KEEPIDLE.0, &int(42))
            .unwrap();
        let host_sock = socket2::SockRef::from(&s.0.sock);
        // Stored, not applied, while keepalive is off.
        assert!(!host_sock.keepalive().unwrap());
        s.set_option(
            SocketLevel::SOL_SOCKET.0,
            SocketOption::SO_KEEPALIVE.0,
            &int(1),
        )
        .unwrap();
        assert!(host_sock.keepalive().unwrap());
        assert_eq!(
            host_sock.tcp_keepalive_time().unwrap(),
            Duration::from_secs(42)
        );
        s.set_option(tcp, TcpOption::TCP_NODELAY.0, &int(1))
            .unwrap();
        assert!(host_sock.tcp_nodelay().unwrap());
        s.set_option(
            SocketLevel::SOL_SOCKET.0,
            SocketOption::SO_KEEPALIVE.0,
            &int(0),
        )
        .unwrap();
        assert!(!host_sock.keepalive().unwrap());
    }

    #[test]
    fn a_datagram_endpoint_is_writable_until_the_host_is_full() {
        let b = backend();
        let wq = WaitQueue::new();
        let d = b.datagram(Domain::Inet, wq.clone()).unwrap();
        assert!(d.poll(None).contains(PollMask::OUT));
        let (tx, rx) = mpsc::channel();
        struct Signal(mpsc::Sender<PollMask>);
        impl WaitEntry for Signal {
            fn wake(&self, events: PollMask) -> bool {
                let _ = self.0.send(events);
                true
            }
        }
        let _reg = wq.add(Arc::new(Signal(tx)));
        // What a send meeting `EAGAIN` leaves.
        let shared = b
            .shared
            .entries
            .lock()
            .unwrap()
            .values()
            .find_map(|e| match e {
                Entry::Datagram(d) => Some(d.clone()),
                _ => None,
            });
        let shared = shared.unwrap();
        shared.state().host_writable = false;
        assert!(!d.poll(None).intersects(PollMask::OUT | PollMask::WRNORM));
        // The reactor's writable event wakes the guest socket's queue with OUT.
        shared.writable();
        assert!(rx.try_recv().unwrap().contains(PollMask::OUT));
        assert!(d.poll(None).contains(PollMask::OUT));
    }

    #[test]
    fn a_persistent_receive_error_does_not_spin_the_reactor() {
        let mut fds = [0; 2];
        // SAFETY: `pipe` fills both descriptors.
        assert_eq!(unsafe { libc_pipe(fds.as_mut_ptr()) }, 0);
        // A descriptor that is not a socket fails every receive with ENOTSOCK.
        // SAFETY: the read end is ours and closed when the socket drops.
        let sock = unsafe { std::net::UdpSocket::from_raw_fd(fds[0]) };
        let shared = DgramShared {
            sock: UdpSocket::from_std(sock),
            token: Token(1),
            wq: WaitQueue::new(),
            st: Mutex::new(DgramState {
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                recverr: [false; 2],
                errqueue: VecDeque::new(),
                connected: Some("127.0.0.1:9".parse().unwrap()),
                host_writable: true,
            }),
            backend: Weak::new(),
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            shared.receive_all(&mut [0u8; 64]);
            let _ = tx.send(shared.state().err);
        });
        let err = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive_all returns");
        assert_eq!(err, Some(Errno::ENOTSOCK));
        // SAFETY: the write end is still open and ours.
        unsafe { libc_close(fds[1]) };
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_port_unreachable_queues_one_error_with_ip_recverr() {
        let b = backend();
        let wq = WaitQueue::new();
        let (tx, rx) = mpsc::channel();
        struct Signal(mpsc::Sender<PollMask>);
        impl WaitEntry for Signal {
            fn wake(&self, events: PollMask) -> bool {
                let _ = self.0.send(events);
                true
            }
        }
        let _reg = wq.add(Arc::new(Signal(tx)));
        let d = b.datagram(Domain::Inet, wq).unwrap();
        d.set_option(
            SocketLevel::SOL_IP.0,
            IpOption::IP_RECVERR.0,
            &1i32.to_le_bytes(),
        )
        .unwrap();
        // A loopback port nobody holds.
        let closed = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let dst = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(d.send_to(b"ping", dst), Ok(4));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the error wakes the guest socket")
            .contains(PollMask::ERR)
        {}
        assert!(d.poll(None).contains(PollMask::ERR));
        // Unconnected, the error is still pending with IP_RECVERR.
        assert_eq!(d.take_error(), Some(Errno::ECONNREFUSED));
        let e = d.take_queued_error().expect("one queued error");
        assert_eq!(e.errno, Errno::ECONNREFUSED);
        assert_eq!(e.origin, wasixv2_abi::SoEeOrigin::SO_EE_ORIGIN_ICMP);
        // ICMP_DEST_UNREACH, ICMP_PORT_UNREACH.
        assert_eq!((e.type_, e.code), (3, 3));
        assert_eq!(e.dst, Some(dst));
        assert_eq!(e.offender.map(|o| o.ip()), Some(dst.ip()));
        assert_eq!(e.payload, b"ping");
        assert_eq!(d.take_queued_error(), None);
        assert!(!d.poll(None).contains(PollMask::ERR));
    }

    unsafe extern "C" {
        #[link_name = "pipe"]
        fn libc_pipe(fds: *mut i32) -> i32;
        #[link_name = "close"]
        fn libc_close(fd: i32) -> i32;
    }
}
