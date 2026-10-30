use crate::{Capabilities, Driver};
use futures_util::task::AtomicWaker;
use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{
    Backend, BackendDatagram, BackendStream, Domain, InboundSink, Lookup, LookupDone, LookupError,
    Publication, PublishedDatagram, RecvQueue, Shutdown, StreamState,
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue};

/// Payload and object limits apply across task workers. Each endpoint has bounded incoming
/// and outgoing storage; datagrams are charged at least one byte, including empty datagrams.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub max_sockets: usize,
    pub max_lookups: usize,
    pub buffer_bytes: usize,
    pub chunk_bytes: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_sockets: 128,
            max_lookups: 64,
            buffer_bytes: 256 * 1024,
            chunk_bytes: 16 * 1024,
        }
    }
}

pub(crate) struct Shared {
    pub config: Config,
    pub inner: Mutex<Inner>,
    pub wake: AtomicWaker,
    pub failures: Mutex<VecDeque<(SocketAddr, Errno)>>,
}
pub(crate) struct Inner {
    pub capabilities: Capabilities,
    pub online: bool,
    pub generation: u64,
    pub next_id: u64,
    pub flows: BTreeMap<u64, Flow>,
    pub listeners: BTreeMap<u64, Listener>,
    pub lookups: BTreeMap<u64, Query>,
}
impl Inner {
    pub fn id(&mut self) -> Result<u64, Errno> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(Errno::ENOBUFS)?;
        Ok(id)
    }
    fn room(&self, config: Config) -> Result<(), Errno> {
        if !self.online {
            return Err(Errno::ENETDOWN);
        }
        if self.flows.len() + self.listeners.len() >= config.max_sockets {
            return Err(Errno::ENOBUFS);
        }
        Ok(())
    }
}
pub(crate) struct Query {
    pub lookup: Lookup,
    pub done: LookupDone,
    pub sent: bool,
}
pub(crate) struct Listener {
    pub addr: SocketAddr,
    pub sink: Arc<dyn InboundSink>,
    pub opening: bool,
    pub ready: bool,
    pub released: bool,
    pub child: Option<u64>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Stream,
    Datagram(Domain),
}
pub(crate) struct Packet {
    pub data: Vec<u8>,
    pub addr: SocketAddr,
}
pub(crate) struct Flow {
    pub kind: Kind,
    pub peer: SocketAddr,
    pub connected_peer: Option<SocketAddr>,
    pub wq: WaitQueue,
    pub state: StreamState,
    pub opening: bool,
    pub released: bool,
    pub reset: bool,
    pub reset_sent: bool,
    pub remote: bool,
    pub error: Option<Errno>,
    pub rx: RecvQueue,
    pub oob_inline: bool,
    pub tx_head: u64,
    pub tx_urgent: VecDeque<u64>,
    pub published: bool,
    pub attached: bool,
    pub tx: VecDeque<u8>,
    pub incoming: VecDeque<Packet>,
    pub outgoing: VecDeque<Packet>,
    pub incoming_bytes: usize,
    pub outgoing_bytes: usize,
    pub inflight: Option<(u64, usize)>,
    pub send_blocked: bool,
    pub read_shut: bool,
    pub peer_fin: bool,
    pub write_shut: bool,
    pub sent_read_shut: bool,
    pub sent_write_shut: bool,
}
impl Flow {
    pub fn new(kind: Kind, peer: SocketAddr, wq: WaitQueue) -> Self {
        Self {
            kind,
            peer,
            connected_peer: None,
            wq,
            state: StreamState::Connecting,
            opening: false,
            released: false,
            reset: false,
            reset_sent: false,
            remote: false,
            error: None,
            rx: RecvQueue::default(),
            oob_inline: false,
            tx_head: 0,
            tx_urgent: VecDeque::new(),
            published: false,
            attached: true,
            tx: VecDeque::new(),
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
            incoming_bytes: 0,
            outgoing_bytes: 0,
            inflight: None,
            send_blocked: false,
            read_shut: false,
            peer_fin: false,
            write_shut: false,
            sent_read_shut: false,
            sent_write_shut: false,
        }
    }
    pub fn fail(&mut self, error: Errno) {
        self.state = StreamState::Closed;
        self.opening = false;
        self.error = Some(error);
        self.read_shut = true;
        self.write_shut = true;
        self.tx.clear();
        self.outgoing.clear();
        self.outgoing_bytes = 0;
        self.inflight = None;
    }
    pub fn mask(&self, config: Config) -> PollMask {
        // Linux net/ipv4/tcp.c:tcp_poll; net/ipv4/udp.c:udp_poll. FIN only ends reading.
        let mut mask = PollMask::empty();
        if self.error.is_some() {
            mask |= PollMask::ERR;
        }
        if self.kind != Kind::Stream {
            if !self.incoming.is_empty() {
                mask |= PollMask::READABLE;
            }
            if self.state == StreamState::Established && self.outgoing_bytes < config.buffer_bytes {
                mask |= PollMask::WRITABLE;
            }
            return mask;
        }
        if self.state == StreamState::Connecting {
            return mask;
        }
        if self.rx.urgent() {
            mask |= PollMask::PRI;
        }
        if self.rx.readable(1, self.oob_inline) || self.read_shut {
            mask |= PollMask::READABLE;
        }
        if (self.tx.len() < config.buffer_bytes && !self.send_blocked) || self.write_shut {
            mask |= PollMask::WRITABLE;
        }
        if self.read_shut {
            mask |= PollMask::RDHUP;
        }
        if self.read_shut && self.write_shut {
            mask |= PollMask::HUP;
        }
        mask
    }
}

/// One network backend per Machine (or isolated network configuration).
pub struct WebBackend {
    pub(crate) shared: Arc<Shared>,
}
impl WebBackend {
    /// Creates a ready transport session. The caller must poll the returned driver on the
    /// I/O worker and pass a transport with exactly these capabilities.
    pub fn new(config: Config, capabilities: Capabilities) -> Result<(Arc<Self>, Driver), Errno> {
        if config.max_sockets == 0
            || config.max_lookups == 0
            || config.buffer_bytes == 0
            || config.chunk_bytes == 0
            || config.chunk_bytes > config.buffer_bytes
            || config.buffer_bytes > crate::codec::MAX_FRAME / 2
        {
            return Err(Errno::EINVAL);
        }
        let shared = Arc::new(Shared {
            config,
            inner: Mutex::new(Inner {
                capabilities,
                online: true,
                generation: 1,
                next_id: 1,
                flows: BTreeMap::new(),
                listeners: BTreeMap::new(),
                lookups: BTreeMap::new(),
            }),
            wake: AtomicWaker::new(),
            failures: Mutex::new(VecDeque::new()),
        });
        Ok((
            Arc::new(Self {
                shared: shared.clone(),
            }),
            Driver::new(shared, 1),
        ))
    }
    /// Asynchronous publication failures are embedder errors, never guest bind errors.
    pub fn take_publication_error(&self) -> Option<(SocketAddr, Errno)> {
        self.shared.failures.lock().unwrap().pop_front()
    }
    pub fn capabilities(&self) -> Capabilities {
        self.shared.inner.lock().unwrap().capabilities
    }

    /// Attach a new session after disconnect or driver cancellation. Existing socket handles
    /// retain their failure; only newly created sockets use the new session.
    pub fn reconnect(&self, capabilities: Capabilities) -> Result<Driver, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        if inner.online {
            return Err(Errno::EBUSY);
        }
        inner.generation = inner.generation.checked_add(1).ok_or(Errno::ENOBUFS)?;
        inner.capabilities = capabilities;
        inner.online = true;
        Ok(Driver::new(self.shared.clone(), inner.generation))
    }
}

fn family_allowed(capabilities: Capabilities, addr: SocketAddr) -> Result<(), Errno> {
    if addr.is_ipv6() && !capabilities.ipv6 {
        Err(Errno::EAFNOSUPPORT)
    } else {
        Ok(())
    }
}
impl Backend for WebBackend {
    fn connect(&self, peer: SocketAddr, wq: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.room(self.shared.config)?;
        if !inner.capabilities.tcp {
            return Err(Errno::EOPNOTSUPP);
        }
        family_allowed(inner.capabilities, peer)?;
        let id = inner.id()?;
        inner.flows.insert(id, Flow::new(Kind::Stream, peer, wq));
        drop(inner);
        self.shared.wake.wake();
        Ok(Arc::new(Stream {
            shared: self.shared.clone(),
            id,
            peer,
        }))
    }
    fn datagram(&self, domain: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.room(self.shared.config)?;
        if !inner.capabilities.udp {
            return Err(Errno::EOPNOTSUPP);
        }
        let peer = unbound(domain);
        family_allowed(inner.capabilities, peer)?;
        let id = inner.id()?;
        inner
            .flows
            .insert(id, Flow::new(Kind::Datagram(domain), peer, wq));
        drop(inner);
        self.shared.wake.wake();
        Ok(Arc::new(Datagram {
            shared: self.shared.clone(),
            id,
        }))
    }
    fn publish(
        &self,
        host: SocketAddr,
        _backlog: u32,
        sink: Arc<dyn InboundSink>,
    ) -> Result<Box<dyn Publication>, std::io::Error> {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.room(self.shared.config).map_err(io_error)?;
        if !inner.capabilities.listen || host.port() == 0 {
            // Classic ListenTcp has no bound-address response for ephemeral publication.
            return Err(std::io::ErrorKind::Unsupported.into());
        }
        family_allowed(inner.capabilities, host).map_err(io_error)?;
        let id = inner.id().map_err(io_error)?;
        inner.listeners.insert(
            id,
            Listener {
                addr: host,
                sink,
                opening: false,
                ready: false,
                released: false,
                child: None,
            },
        );
        drop(inner);
        self.shared.wake.wake();
        Ok(Box::new(Published {
            shared: self.shared.clone(),
            id,
            host,
        }))
    }
    fn publish_datagram(&self, host: SocketAddr) -> Result<PublishedDatagram, std::io::Error> {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.room(self.shared.config).map_err(io_error)?;
        if !inner.capabilities.udp_publish || host.port() == 0 {
            return Err(std::io::ErrorKind::Unsupported.into());
        }
        family_allowed(inner.capabilities, host).map_err(io_error)?;
        let id = inner.id().map_err(io_error)?;
        let domain = if host.is_ipv4() {
            Domain::Inet
        } else {
            Domain::Inet6
        };
        let mut flow = Flow::new(Kind::Datagram(domain), host, WaitQueue::new());
        flow.published = true;
        flow.attached = false;
        inner.flows.insert(id, flow);
        drop(inner);
        self.shared.wake.wake();
        let endpoint = Arc::new(Datagram {
            shared: self.shared.clone(),
            id,
        });
        Ok((
            endpoint.clone(),
            Box::new(DatagramPublication { endpoint, host }),
        ))
    }
    fn lookup(&self, lookup: Lookup, done: LookupDone) {
        let mut inner = self.shared.inner.lock().unwrap();
        let caps = inner.capabilities;
        let supported = match &lookup {
            Lookup::Name(name) => caps.dns_name && name.len() <= 253 && !name.contains('\0'),
            Lookup::Addr(_) => caps.dns_reverse,
            Lookup::Forward(message) => caps.dns_forward && message.len() <= 65535,
        };
        if !inner.online || !supported || inner.lookups.len() >= self.shared.config.max_lookups {
            drop(inner);
            done(Err(LookupError::Failed));
            return;
        }
        let Ok(request) = inner.id() else {
            drop(inner);
            done(Err(LookupError::Failed));
            return;
        };
        inner.lookups.insert(
            request,
            Query {
                lookup,
                done,
                sent: false,
            },
        );
        drop(inner);
        self.shared.wake.wake();
    }
}
fn io_error(error: Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error.0)
}
pub(crate) fn unbound(domain: Domain) -> SocketAddr {
    match domain {
        Domain::Inet => "0.0.0.0:0".parse().unwrap(),
        Domain::Inet6 => "[::]:0".parse().unwrap(),
    }
}

pub(crate) struct Stream {
    pub shared: Arc<Shared>,
    pub id: u64,
    pub peer: SocketAddr,
}
impl Pollable for Stream {
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        let wq = self.shared.inner.lock().unwrap().flows[&self.id].wq.clone();
        if let Some(table) = table {
            table.wait_on(&wq);
        }
        self.shared.inner.lock().unwrap().flows[&self.id].mask(self.shared.config)
    }
}
impl Stream {
    fn enqueue(&self, data: &[u8], urgent: bool) -> Result<usize, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let flow = inner.flows.get_mut(&self.id).unwrap();
        // Linux net/ipv4/tcp.c:tcp_sendmsg_locked consumes sk_err before EPIPE.
        if let Some(error) = flow.error.take() {
            return Err(error);
        }
        if flow.write_shut || flow.state == StreamState::Closed {
            return Err(Errno::EPIPE);
        }
        if flow.state != StreamState::Established {
            return Err(Errno::EAGAIN);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let n = data
            .len()
            .min(self.shared.config.buffer_bytes - flow.tx.len());
        if n == 0 {
            return Err(Errno::EAGAIN);
        }
        if urgent {
            flow.tx_urgent
                .push_back(flow.tx_head + flow.tx.len() as u64 + n as u64 - 1);
        }
        flow.tx.extend(&data[..n]);
        drop(inner);
        self.shared.wake.wake();
        Ok(n)
    }
}
impl BackendStream for Stream {
    fn state(&self) -> StreamState {
        self.shared.inner.lock().unwrap().flows[&self.id].state
    }
    fn tcp_established(&self) -> bool {
        let inner = self.shared.inner.lock().unwrap();
        let flow = &inner.flows[&self.id];
        flow.state == StreamState::Established && !flow.peer_fin && !flow.write_shut
    }
    fn read(&self, buf: &mut [u8], peek: bool) -> Result<usize, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let flow = inner.flows.get_mut(&self.id).unwrap();
        if buf.is_empty() {
            return Ok(0);
        }
        // Linux net/ipv4/tcp.c:tcp_recvmsg_locked drains queued bytes before sk_err/EOF.
        let queued = flow.rx.len();
        let n = flow.rx.read(buf, peek, flow.oob_inline);
        let consumed = flow.rx.len() != queued;
        let result = if n != 0 {
            Ok(n)
        } else if let Some(error) = flow.error.take() {
            Err(error)
        } else if flow.read_shut {
            Ok(0)
        } else {
            Err(Errno::EAGAIN)
        };
        drop(inner);
        // tcp_recvmsg_locked consumes a skipped non-inline urgent byte too. Reopen the
        // driver's receive window even when no ordinary bytes were returned to the caller.
        if consumed {
            self.shared.wake.wake();
        }
        result
    }

    fn write(&self, data: &[u8]) -> Result<usize, Errno> {
        self.enqueue(data, false)
    }
    fn write_urgent(&self, data: &[u8]) -> Result<usize, Errno> {
        if !self.shared.inner.lock().unwrap().capabilities.urgent {
            return Err(Errno::EOPNOTSUPP);
        }
        // Linux net/ipv4/tcp.c:tcp_sendmsg_locked sets the mark on the last accepted byte.
        // Queue bytes and mark under one lock; an I/O worker cannot send before publication.
        self.enqueue(data, true)
    }
    fn read_urgent(&self, peek: bool) -> Result<u8, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let f = inner.flows.get_mut(&self.id).unwrap();
        f.rx.recv_urgent(
            peek,
            f.oob_inline,
            f.state == StreamState::Closed && !f.peer_fin,
        )
    }
    fn at_mark(&self) -> bool {
        self.shared.inner.lock().unwrap().flows[&self.id]
            .rx
            .at_mark()
    }
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let flow = inner.flows.get_mut(&self.id).unwrap();
        if flow.state != StreamState::Established {
            return Err(Errno::ENOTCONN);
        }
        // Linux net/ipv4/tcp.c:tcp_shutdown queues FIN after bytes already accepted.
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            // Linux tcp_shutdown does not discard the receive queue or send a FIN for
            // SHUT_RD; tcp_recvmsg_locked still returns bytes already accepted.
            flow.read_shut = true;
        }
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            flow.write_shut = true;
        }
        let wq = flow.wq.clone();
        drop(inner);
        wq.wake(PollMask::READABLE | PollMask::WRITABLE | PollMask::RDHUP | PollMask::HUP);
        self.shared.wake.wake();
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        self.shared
            .inner
            .lock()
            .unwrap()
            .flows
            .get_mut(&self.id)
            .unwrap()
            .error
            .take()
    }
    fn peer_addr(&self) -> SocketAddr {
        self.peer
    }
    fn input_len(&self) -> usize {
        let inner = self.shared.inner.lock().unwrap();
        let f = &inner.flows[&self.id];
        f.rx.inq(f.oob_inline)
    }
    fn output_len(&self) -> usize {
        self.shared.inner.lock().unwrap().flows[&self.id].tx.len()
    }
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        if level != wasixv2_abi::SocketLevel::SOL_SOCKET.0
            || name != wasixv2_abi::SocketOption::SO_OOBINLINE.0
        {
            return Err(Errno::EOPNOTSUPP);
        }
        let value: [u8; 4] = value.try_into().map_err(|_| Errno::EINVAL)?;
        self.shared
            .inner
            .lock()
            .unwrap()
            .flows
            .get_mut(&self.id)
            .unwrap()
            .oob_inline = i32::from_le_bytes(value) != 0;
        Ok(())
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        release(&self.shared, self.id);
    }
}

struct Datagram {
    shared: Arc<Shared>,
    id: u64,
}
impl Pollable for Datagram {
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        let wq = self.shared.inner.lock().unwrap().flows[&self.id].wq.clone();
        if let Some(table) = table {
            table.wait_on(&wq);
        }
        self.shared.inner.lock().unwrap().flows[&self.id].mask(self.shared.config)
    }
}
impl BackendDatagram for Datagram {
    fn send_to(&self, data: &[u8], to: SocketAddr) -> Result<usize, Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        family_allowed(inner.capabilities, to)?;
        let flow = inner.flows.get_mut(&self.id).unwrap();
        if let Some(error) = flow.error.take() {
            return Err(error);
        }
        if flow.state == StreamState::Closed {
            return Err(Errno::ENETDOWN);
        }
        if flow.state != StreamState::Established {
            return Err(Errno::EAGAIN);
        }
        if let Kind::Datagram(domain) = flow.kind
            && (domain == Domain::Inet) != to.is_ipv4()
        {
            return Err(Errno::EAFNOSUPPORT);
        }
        if data.len() > 65507 || data.len().max(1) > self.shared.config.buffer_bytes {
            return Err(Errno::EMSGSIZE);
        }
        if flow.outgoing_bytes + data.len().max(1) > self.shared.config.buffer_bytes {
            return Err(Errno::EAGAIN);
        }
        flow.outgoing.push_back(Packet {
            data: data.to_vec(),
            addr: to,
        });
        flow.outgoing_bytes += data.len().max(1);
        drop(inner);
        self.shared.wake.wake();
        Ok(data.len())
    }
    fn connect(&self, peer: Option<SocketAddr>) -> Result<(), Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some(peer) = peer {
            family_allowed(inner.capabilities, peer)?;
        }
        let flow = inner.flows.get_mut(&self.id).unwrap();
        // Linux net/ipv4/datagram.c:__ip4_datagram_connect changes future demultiplexing,
        // not datagrams already queued before connect. vnet owns guest peer bookkeeping.
        flow.connected_peer = peer;
        Ok(())
    }
    fn take_error(&self) -> Option<Errno> {
        self.shared
            .inner
            .lock()
            .unwrap()
            .flows
            .get_mut(&self.id)
            .unwrap()
            .error
            .take()
    }
    fn recv_from(&self, buf: &mut [u8], peek: bool) -> Result<(usize, usize, SocketAddr), Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let flow = inner.flows.get_mut(&self.id).unwrap();
        // Linux net/ipv4/udp.c:udp_recvmsg preserves datagram length for MSG_TRUNC.
        if let Some(error) = flow.error.take() {
            return Err(error);
        }
        let Some(packet) = flow.incoming.front() else {
            return Err(Errno::EAGAIN);
        };
        let n = packet.data.len().min(buf.len());
        buf[..n].copy_from_slice(&packet.data[..n]);
        let result = (n, packet.data.len(), packet.addr);
        if !peek {
            flow.incoming_bytes -= packet.data.len().max(1);
            flow.incoming.pop_front();
        }
        Ok(result)
    }
    fn attach(&self, wq: Option<WaitQueue>) -> Result<(), Errno> {
        let mut inner = self.shared.inner.lock().unwrap();
        let f = inner.flows.get_mut(&self.id).unwrap();
        if !f.published {
            return Err(Errno::EOPNOTSUPP);
        }
        // Linux net/ipv4/udp.c:udp_lib_close: a new socket inherits no queued data/errors.
        f.incoming.clear();
        f.incoming_bytes = 0;
        f.outgoing.clear();
        f.outgoing_bytes = 0;
        f.error = None;
        f.connected_peer = None;
        f.attached = wq.is_some();
        f.wq = wq.unwrap_or_default();
        Ok(())
    }
    fn input_len(&self) -> usize {
        self.shared.inner.lock().unwrap().flows[&self.id]
            .incoming
            .front()
            .map_or(0, |p| p.data.len())
    }
    fn set_option(&self, _: i32, _: i32, _: &[u8]) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }
}
impl Drop for Datagram {
    fn drop(&mut self) {
        release(&self.shared, self.id);
    }
}

fn release(shared: &Arc<Shared>, id: u64) {
    let mut inner = shared.inner.lock().unwrap();
    let Some(flow) = inner.flows.get_mut(&id) else {
        return;
    };
    flow.released = true;
    // Linux net/ipv4/tcp.c:tcp_close aborts on unread data, otherwise drains before FIN.
    flow.reset |= !flow.rx.is_empty();
    flow.rx.clear();
    flow.incoming.clear();
    flow.incoming_bytes = 0;
    flow.write_shut = true;
    if !flow.remote && flow.state == StreamState::Closed {
        inner.flows.remove(&id);
    }
    drop(inner);
    shared.wake.wake();
}
struct Published {
    shared: Arc<Shared>,
    id: u64,
    host: SocketAddr,
}
impl Publication for Published {
    fn host_addr(&self) -> SocketAddr {
        self.host
    }
    fn set_backlog(&self, _backlog: u32) {
        self.shared.wake.wake();
    }
}
impl Drop for Published {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some(listener) = inner.listeners.get_mut(&self.id) {
            listener.released = true;
        }
        drop(inner);
        self.shared.wake.wake();
    }
}

struct DatagramPublication {
    endpoint: Arc<Datagram>,
    host: SocketAddr,
}
impl Publication for DatagramPublication {
    fn host_addr(&self) -> SocketAddr {
        self.host
    }
    fn set_backlog(&self, _: u32) {
        let _ = &self.endpoint;
    }
}
