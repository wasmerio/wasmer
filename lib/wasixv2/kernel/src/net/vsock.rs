//! `AF_VSOCK` sockets (`net/vmw_vsock/af_vsock.c`), stream and seqpacket, with the transports a
//! machine has:
//!
//! - **Loopback** (`vsock_loopback.c`): `VMADDR_CID_LOCAL` and the machine's own CID connect
//!   to listeners in the kernel, over an in-memory connection with virtio-vsock's credit,
//!   shutdown and reset rules (`virtio_transport_common.c`).
//! - **Guest to host** (virtio-vsock's guest side): when the network's backend carries vsock
//!   ([`wasixv2_vnet::Backend::vsock_cid`]), the machine is a guest with that CID, and every
//!   other CID (the hypervisor, the host, a sibling through `VMADDR_FLAG_TO_HOST` or the
//!   `g2h_fallback` default) is the backend's to reach. Without one the machine is a host with
//!   only the loopback transport: its local CID is 1, and the host CID 2 is itself.
//!
//! Every namespace is in vsock's "global" mode (the only one before a namespace manager writes
//! `child_ns_mode`): sockets of all network namespaces share one bind table. There is no
//! datagram transport, so `SOCK_DGRAM` fails with `ENODEV`; the host bridge carries streams
//! only, so a seqpacket socket reaches the host as a device without `VIRTIO_VSOCK_F_SEQPACKET`
//! (`ESOCKTNOSUPPORT`). `/dev/vsock` answers `IOCTL_VM_SOCKETS_GET_LOCAL_CID`.
//!
//! Not yet: connections the host opens to a guest listener, the `net.vsock` sysctls,
//! `SO_ZEROCOPY`, `SO_INQ` and the error queue.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::Duration,
};

use wasixv2_abi::{
    AddressFamily, GuestLayout, GuestPtrMut, IoctlRequest, MsgFlags, OpenFlags, SockaddrVm,
    SocketFlags, SocketLevel, SocketOption, SocketType, Vmaddr, VmaddrFlags, VsockOption,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vfs::Cap;
use wasixv2_vnet::{
    BackendStream, Network, Shutdown, SocketKind, StreamState,
    opts::{self, SockOpts},
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    ns::NetNs,
    socket::{Blocking, RCV, SND, Timeouts, WaitOn},
    sync::MutexExt,
};

const CID_LOCAL: u32 = Vmaddr::VMADDR_CID_LOCAL.0;
const CID_HOST: u32 = Vmaddr::VMADDR_CID_HOST.0;
const CID_ANY: u32 = Vmaddr::VMADDR_CID_ANY.0;
const PORT_ANY: u32 = Vmaddr::VMADDR_PORT_ANY.0;
const FLAG_TO_HOST: u8 = VmaddrFlags::VMADDR_FLAG_TO_HOST.bits();
/// Ports up to this one need `CAP_NET_BIND_SERVICE`.
const LAST_RESERVED_PORT: u32 = 1023;
const MAX_PORT_RETRIES: usize = 24;
const RCV_SHUTDOWN: u8 = 1;
const SEND_SHUTDOWN: u8 = 2;
const SHUTDOWN_MASK: u8 = 3;
/// `VSOCK_DEFAULT_CONNECT_TIMEOUT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// `VSOCK_DEFAULT_BUFFER_SIZE`, `_MAX_SIZE` and `_MIN_SIZE`.
const BUFFER_SIZE: u64 = 256 * 1024;
const BUFFER_MIN: u64 = 128;
/// `MAX_SCHEDULE_TIMEOUT / HZ - 1` at `HZ` 1000: the longest connect timeout.
const MAX_TIMEOUT_SECS: i64 = (i64::MAX / 1000) - 1;

/// A vsock address: a context id, a port and `svm_flags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Addr {
    pub(crate) cid: u32,
    pub(crate) port: u32,
    flags: u8,
}

impl Addr {
    const UNBOUND: Addr = Addr::new(CID_ANY, PORT_ANY);

    const fn new(cid: u32, port: u32) -> Self {
        Self {
            cid,
            port,
            flags: 0,
        }
    }

    /// `vsock_addr_cast`: a whole `sockaddr_vm` of the family, with no unknown flags.
    fn cast(raw: &[u8]) -> Result<Self, Errno> {
        let vm = SockaddrVm::read_from(raw.get(..SockaddrVm::SIZE).ok_or(Errno::EFAULT)?)
            .ok_or(Errno::EFAULT)?;
        if i32::from(vm.svm_family) != AddressFamily::AF_VSOCK.0 {
            return Err(Errno::EAFNOSUPPORT);
        }
        if vm.svm_flags & !FLAG_TO_HOST != 0 {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            cid: vm.svm_cid,
            port: vm.svm_port,
            flags: vm.svm_flags,
        })
    }

    /// `vsock_addr_bound`.
    fn bound(&self) -> bool {
        self.port != PORT_ANY
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let vm = SockaddrVm {
            svm_family: AddressFamily::AF_VSOCK.0 as u16,
            svm_reserved1: 0,
            svm_port: self.port,
            svm_cid: self.cid,
            svm_flags: self.flags,
            svm_zero: [0; 3],
        };
        let mut out = vec![0; SockaddrVm::SIZE];
        vm.write_to(&mut out);
        out
    }
}

/// The transports of the machine a socket lives on.
#[derive(Clone)]
pub(crate) struct Transports {
    /// The guest-to-host transport: the CID the backend gives the machine, and the network
    /// whose backend carries it.
    g2h: Option<(u32, Arc<dyn Network>)>,
}

/// Where a connection to a CID goes (`vsock_assign_transport`).
enum Route {
    Local,
    Host(Arc<dyn Network>),
}

impl Transports {
    pub(crate) fn new(network: Option<&Arc<dyn Network>>) -> Self {
        let g2h = network.and_then(|n| n.vsock_cid().map(|cid| (cid, n.clone())));
        Self { g2h }
    }

    /// `IOCTL_VM_SOCKETS_GET_LOCAL_CID`: the g2h transport's CID, else the loopback's.
    pub(crate) fn local_cid(&self) -> u32 {
        self.g2h.as_ref().map_or(CID_LOCAL, |(cid, _)| *cid)
    }

    /// `vsock_find_cid`: a CID a socket may bind to.
    fn has_cid(&self, cid: u32) -> bool {
        cid == CID_LOCAL || self.g2h.as_ref().is_some_and(|(own, _)| *own == cid)
    }

    /// `vsock_use_local_transport`, then g2h (for the host's CIDs, `VMADDR_FLAG_TO_HOST`, and
    /// with `g2h_fallback` every CID, as virtio's `has_remote_cid` claims them all), else no
    /// transport (`ENODEV`). A sibling CID reached through the host gets the flag.
    fn route(&self, remote: &mut Addr) -> Result<Route, Errno> {
        let local = match &self.g2h {
            Some((own, _)) => remote.cid == *own,
            None => remote.cid == CID_HOST,
        };
        if remote.cid == CID_LOCAL || local {
            return Ok(Route::Local);
        }
        let (_, network) = self.g2h.as_ref().ok_or(Errno::ENODEV)?;
        if remote.cid > CID_HOST {
            remote.flags |= FLAG_TO_HOST;
        }
        Ok(Route::Host(network.clone()))
    }
}

/// The bind table every socket of the machine shares (`vsock_bind_table`), and the next port
/// autobind tries (`net->vsock.port`).
#[derive(Default)]
pub(crate) struct Table(Mutex<TableInner>);

#[derive(Default)]
struct TableInner {
    bound: HashMap<u32, Vec<(Addr, Weak<VsockSock>)>>,
    next: u32,
}

impl TableInner {
    /// `__vsock_find_bound_socket_net`: the same address, or the port with either CID any.
    fn find(&self, addr: &Addr) -> Option<Arc<VsockSock>> {
        self.bound
            .get(&addr.port)?
            .iter()
            .filter(|(at, _)| at.cid == addr.cid || at.cid == CID_ANY || addr.cid == CID_ANY)
            .find_map(|(_, sock)| sock.upgrade())
    }
}

impl Table {
    /// `__vsock_bind_connectible`.
    fn bind(
        &self,
        sock: &Arc<VsockSock>,
        addr: Addr,
        privileged: bool,
        random: impl FnOnce() -> u32,
    ) -> Result<Addr, Errno> {
        let mut t = self.0.lock_or_recover();
        if t.next == 0 {
            t.next = LAST_RESERVED_PORT + 1 + random() % (u32::MAX - LAST_RESERVED_PORT - 1);
        }
        let mut new = Addr::new(addr.cid, addr.port);
        if addr.port == PORT_ANY {
            let mut found = false;
            for _ in 0..MAX_PORT_RETRIES {
                if t.next == PORT_ANY || t.next <= LAST_RESERVED_PORT {
                    t.next = LAST_RESERVED_PORT + 1;
                }
                new.port = t.next;
                t.next = t.next.wrapping_add(1);
                if t.find(&new).is_none() {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(Errno::EADDRNOTAVAIL);
            }
        } else {
            if addr.port <= LAST_RESERVED_PORT && !privileged {
                return Err(Errno::EACCES);
            }
            if t.find(&new).is_some() {
                return Err(Errno::EADDRINUSE);
            }
        }
        let entries = t.bound.entry(new.port).or_default();
        entries.retain(|(_, s)| s.strong_count() > 0);
        entries.push((new, Arc::downgrade(sock)));
        Ok(new)
    }

    fn find(&self, addr: &Addr) -> Option<Arc<VsockSock>> {
        self.0.lock_or_recover().find(addr)
    }

    /// `vsock_remove_bound`.
    fn unbind(&self, addr: &Addr, sock: &VsockSock) {
        let mut t = self.0.lock_or_recover();
        if let Some(entries) = t.bound.get_mut(&addr.port) {
            entries.retain(|(_, s)| s.strong_count() > 0 && !std::ptr::eq(s.as_ptr(), sock));
            if entries.is_empty() {
                t.bound.remove(&addr.port);
            }
        }
    }
}

/// A loopback connection between two sockets: side 0 connected, side 1 was accepted. Each
/// side's queue holds what the other sent; a sender has credit for what its peer's buffer
/// (`buf_alloc`, the peer's `SO_VM_SOCKETS_BUFFER_SIZE`) has free, as virtio-vsock's
/// `virtio_transport_get_credit` counts it, and a `SHUTDOWN` a side sent is its peer's
/// `peer_shutdown`. Releasing a socket sends `SHUTDOWN_MASK` and drops what it had queued.
struct Pipe {
    inner: Mutex<PipeInner>,
    wq: [WaitQueue; 2],
}

#[derive(Default)]
struct PipeInner {
    /// Messages for a seqpacket connection, written chunks for a stream one; the first may be
    /// partly read (`head[i]` bytes of it).
    rx: [VecDeque<Vec<u8>>; 2],
    head: [usize; 2],
    bytes: [usize; 2],
    buf_alloc: [usize; 2],
    shut: [u8; 2],
}

impl Pipe {
    fn new(wq: [WaitQueue; 2], buf_alloc: [usize; 2]) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(PipeInner {
                buf_alloc,
                ..PipeInner::default()
            }),
            wq,
        })
    }

    fn lock(&self) -> MutexGuard<'_, PipeInner> {
        self.inner.lock_or_recover()
    }

    fn peer_shutdown(&self, side: usize) -> u8 {
        self.lock().shut[1 - side]
    }

    /// `virtio_transport_stream_has_space`.
    fn space(&self, side: usize) -> usize {
        let g = self.lock();
        g.buf_alloc[1 - side].saturating_sub(g.bytes[1 - side])
    }

    fn peer_buf_alloc(&self, side: usize) -> usize {
        self.lock().buf_alloc[1 - side]
    }

    /// `stream_has_data` (bytes) or `seqpacket_has_data` (messages).
    fn has_data(&self, side: usize, record: bool) -> usize {
        let g = self.lock();
        match record {
            true => g.rx[side].len(),
            false => g.bytes[side],
        }
    }

    fn bytes(&self, side: usize) -> usize {
        self.lock().bytes[side]
    }

    /// `stream_enqueue` / `seqpacket_enqueue` of what the credit allows: all of a message
    /// or nothing (`EAGAIN`), a message larger than the peer's whole buffer `EMSGSIZE`.
    fn write(&self, side: usize, data: &[u8], record: bool) -> Result<usize, Errno> {
        let mut g = self.lock();
        let peer = 1 - side;
        if record && data.len() > g.buf_alloc[peer] {
            return Err(Errno::EMSGSIZE);
        }
        let space = g.buf_alloc[peer].saturating_sub(g.bytes[peer]);
        let n = match record {
            true if space < data.len() => return Err(Errno::EAGAIN),
            true => data.len(),
            false => space.min(data.len()),
        };
        if n == 0 && !data.is_empty() {
            return Err(Errno::EAGAIN);
        }
        if n > 0 || record {
            g.rx[peer].push_back(data[..n].to_vec());
            g.bytes[peer] += n;
        }
        drop(g);
        self.wq[peer].wake(PollMask::IN | PollMask::RDNORM);
        Ok(n)
    }

    /// A stream read: up to `buf.len()` bytes across chunks.
    fn read_stream(&self, side: usize, buf: &mut [u8], peek: bool) -> usize {
        let mut g = self.lock();
        let mut n = 0;
        let mut head = g.head[side];
        let mut taken = 0;
        for chunk in g.rx[side].iter() {
            let part = &chunk[head..];
            let k = part.len().min(buf.len() - n);
            buf[n..n + k].copy_from_slice(&part[..k]);
            n += k;
            if k < part.len() {
                head += k;
                break;
            }
            taken += 1;
            head = 0;
            if n == buf.len() {
                break;
            }
        }
        if !peek && n > 0 {
            g.rx[side].drain(..taken);
            g.head[side] = head;
            g.bytes[side] -= n;
            drop(g);
            self.wq[1 - side].wake(PollMask::OUT | PollMask::WRNORM);
        }
        n
    }

    /// `virtio_transport_seqpacket_do_dequeue`: one message, cut to `buf`, and its length.
    fn read_record(&self, side: usize, buf: &mut [u8], peek: bool) -> Option<(usize, usize)> {
        let mut g = self.lock();
        let msg = g.rx[side].front()?;
        let n = msg.len().min(buf.len());
        buf[..n].copy_from_slice(&msg[..n]);
        let len = msg.len();
        if !peek {
            g.rx[side].pop_front();
            g.bytes[side] -= len;
            drop(g);
            self.wq[1 - side].wake(PollMask::OUT | PollMask::WRNORM);
        }
        Some((n, len))
    }

    /// `virtio_transport_shutdown`: the peer learns `mode` as its `peer_shutdown`.
    fn shutdown(&self, side: usize, mode: u8) {
        self.lock().shut[side] |= mode;
        self.wq[1 - side].wake(PollMask::empty());
        self.wq[side].wake(PollMask::empty());
    }

    /// `virtio_transport_release`: `SHUTDOWN_MASK`, and what was queued for this side is gone.
    fn close(&self, side: usize) {
        let mut g = self.lock();
        g.shut[side] = SHUTDOWN_MASK;
        g.rx[side].clear();
        g.bytes[side] = 0;
        g.head[side] = 0;
        drop(g);
        self.wq[1 - side].wake(PollMask::empty());
    }

    /// `virtio_transport_notify_buffer_size`: the peer's credit follows.
    fn set_buf_alloc(&self, side: usize, size: usize) {
        self.lock().buf_alloc[side] = size;
        self.wq[1 - side].wake(PollMask::OUT | PollMask::WRNORM);
    }
}

/// What a connected socket's data goes through.
enum Link {
    Local { pipe: Arc<Pipe>, side: usize },
    Host(Arc<dyn BackendStream>),
}

/// `sk_state`, with the TCP names af_vsock uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Close,
    Listen,
    SynSent,
    Established,
}

/// `sock->state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SockState {
    Unconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Stream,
    SeqPacket,
}

struct Inner {
    local: Addr,
    remote: Addr,
    state: State,
    sock_state: SockState,
    /// `sk_shutdown`.
    shutdown: u8,
    /// `sk_err`.
    err: Option<Errno>,
    link: Option<Link>,
    accept: VecDeque<Arc<VsockSock>>,
    backlog: u32,
    /// `SO_VM_SOCKETS_BUFFER_SIZE`, `_MIN_SIZE` and `_MAX_SIZE`.
    buffer: u64,
    buffer_min: u64,
    buffer_max: u64,
    connect_timeout: Duration,
    /// The `SOL_SOCKET` options `sock_setsockopt` keeps.
    opts: SockOpts,
}

/// One `AF_VSOCK` socket.
pub(crate) struct VsockSock {
    kind: Kind,
    me: Weak<VsockSock>,
    table: Arc<Table>,
    transports: Transports,
    /// The network namespace the socket was made in (`sock_net`).
    pub(crate) net: Arc<NetNs>,
    inner: Mutex<Inner>,
    wait: WaitQueue,
    pub(crate) timeouts: Timeouts,
    host: Arc<dyn Host>,
}

/// The socket an open file holds; dropping it releases the socket (`vsock_release`).
pub(crate) struct VsockFile(pub(crate) Arc<VsockSock>);

impl Drop for VsockFile {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub(crate) fn downcast(object: Arc<dyn FileObject>) -> Option<Arc<VsockSock>> {
    let any: Arc<dyn std::any::Any + Send + Sync> = object;
    any.downcast::<VsockFile>().ok().map(|f| f.0.clone())
}

impl VsockSock {
    fn new(
        kind: Kind,
        table: Arc<Table>,
        transports: Transports,
        net: Arc<NetNs>,
        host: Arc<dyn Host>,
        parent: Option<&Inner>,
    ) -> Arc<Self> {
        let opts_kind = match kind {
            Kind::Stream => SocketKind::Stream,
            Kind::SeqPacket => SocketKind::Datagram,
        };
        let (buffer, buffer_min, buffer_max, connect_timeout, opts) = match parent {
            Some(p) => (
                p.buffer,
                p.buffer_min,
                p.buffer_max,
                p.connect_timeout,
                p.opts.inherit(),
            ),
            None => (
                BUFFER_SIZE,
                BUFFER_MIN,
                BUFFER_SIZE,
                CONNECT_TIMEOUT,
                SockOpts::unix(opts_kind),
            ),
        };
        Arc::new_cyclic(|me| Self {
            kind,
            me: me.clone(),
            table,
            transports,
            net,
            inner: Mutex::new(Inner {
                local: Addr::UNBOUND,
                remote: Addr::UNBOUND,
                state: State::Close,
                sock_state: SockState::Unconnected,
                shutdown: 0,
                err: None,
                link: None,
                accept: VecDeque::new(),
                backlog: 0,
                buffer,
                buffer_min,
                buffer_max,
                connect_timeout,
                opts,
            }),
            wait: WaitQueue::new(),
            timeouts: Timeouts::default(),
            host,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_or_recover()
    }

    fn arc(&self) -> Arc<Self> {
        self.me.upgrade().expect("a socket in use is alive")
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn queue(&self) -> &WaitQueue {
        &self.wait
    }

    pub(crate) fn socket_type(&self) -> SocketType {
        match self.kind {
            Kind::Stream => SocketType::SOCK_STREAM,
            Kind::SeqPacket => SocketType::SOCK_SEQPACKET,
        }
    }

    pub(crate) fn blocking<'a>(&'a self, io: Io<'a>, dir: usize, flags: MsgFlags) -> Blocking<'a> {
        Blocking::new(&*self.host, io, flags, self.timeouts.get(dir))
    }

    /// A host connection under way that finished: established, or reset (`sk_err`,
    /// `SS_UNCONNECTED`, as `virtio_transport_recv_connecting` leaves a refused socket).
    fn refresh(inner: &mut Inner) {
        if inner.state != State::SynSent {
            return;
        }
        let Some(Link::Host(stream)) = &inner.link else {
            return;
        };
        match stream.state() {
            StreamState::Connecting => {}
            StreamState::Established => {
                inner.state = State::Established;
                inner.sock_state = SockState::Connected;
            }
            StreamState::Closed => {
                inner.err = Some(stream.take_error().unwrap_or(Errno::ECONNRESET));
                inner.state = State::Close;
                inner.sock_state = SockState::Unconnected;
                inner.link = None;
            }
        }
    }

    /// `peer_shutdown`.
    fn peer_shutdown(inner: &Inner) -> u8 {
        match &inner.link {
            Some(Link::Local { pipe, side }) => pipe.peer_shutdown(*side),
            Some(Link::Host(stream)) => {
                let mask = stream.poll(None);
                let mut shut = 0;
                if mask.contains(PollMask::RDHUP) && stream.input_len() == 0 {
                    shut |= SEND_SHUTDOWN;
                }
                if stream.state() == StreamState::Closed {
                    shut = SHUTDOWN_MASK;
                }
                shut
            }
            None => 0,
        }
    }

    /// `SOCK_DONE`: the connection is over (`virtio_transport_do_close`): the peer shut both
    /// directions down or reset it.
    fn done(inner: &Inner) -> bool {
        inner.link.is_some() && Self::peer_shutdown(inner) == SHUTDOWN_MASK
    }

    fn has_data(&self, inner: &Inner) -> usize {
        match &inner.link {
            Some(Link::Local { pipe, side }) => pipe.has_data(*side, self.kind == Kind::SeqPacket),
            Some(Link::Host(stream)) => stream.input_len(),
            None => 0,
        }
    }

    pub(crate) fn local_addr(&self) -> Vec<u8> {
        self.lock().local.encode()
    }

    /// `vsock_getname` for the peer: `ENOTCONN` unless connected.
    pub(crate) fn peer_addr(&self) -> Result<Vec<u8>, Errno> {
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        match inner.sock_state {
            SockState::Connected => Ok(inner.remote.encode()),
            _ => Err(Errno::ENOTCONN),
        }
    }

    /// `vsock_bind`: a malformed address is `EINVAL` whatever was wrong with it.
    pub(crate) fn bind(&self, raw: &[u8], privileged: bool) -> Result<(), Errno> {
        let addr = Addr::cast(raw).map_err(|_| Errno::EINVAL)?;
        let mut inner = self.lock();
        self.bind_locked(&mut inner, addr, privileged)
    }

    /// `__vsock_bind`: once only, to a CID of the machine or any.
    fn bind_locked(&self, inner: &mut Inner, addr: Addr, privileged: bool) -> Result<(), Errno> {
        if inner.local.bound() {
            return Err(Errno::EINVAL);
        }
        if addr.cid != CID_ANY && !self.transports.has_cid(addr.cid) {
            return Err(Errno::EADDRNOTAVAIL);
        }
        let host = self.host.clone();
        let random = move || {
            let mut b = [0; 4];
            host.random(&mut b);
            u32::from_le_bytes(b)
        };
        inner.local = self.table.bind(&self.arc(), addr, privileged, random)?;
        Ok(())
    }

    /// `vsock_listen`.
    pub(crate) fn listen(&self, backlog: u32) -> Result<(), Errno> {
        let mut inner = self.lock();
        if inner.sock_state != SockState::Unconnected || !inner.local.bound() {
            return Err(Errno::EINVAL);
        }
        inner.backlog = backlog;
        inner.state = State::Listen;
        drop(inner);
        self.wait.wake(PollMask::empty());
        Ok(())
    }

    /// `vsock_accept` without waiting.
    pub(crate) fn try_accept(&self) -> Result<Arc<VsockSock>, Errno> {
        let mut inner = self.lock();
        if inner.state != State::Listen {
            return Err(Errno::EINVAL);
        }
        let child = inner.accept.pop_front().ok_or(Errno::EAGAIN)?;
        drop(inner);
        child.lock().sock_state = SockState::Connected;
        Ok(child)
    }

    /// `vsock_connect` up to the wait: `Ok(true)` once connected, `Ok(false)` while a
    /// connection is under way, which the caller waits for or reports as `EINPROGRESS`.
    pub(crate) fn start_connect(&self, raw: &[u8], nonblocking: bool) -> Result<bool, Errno> {
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        match inner.sock_state {
            SockState::Connected => return Err(Errno::EISCONN),
            SockState::Disconnecting => return Err(Errno::EINVAL),
            SockState::Connecting if nonblocking => return Err(Errno::EALREADY),
            SockState::Connecting => return Ok(false),
            SockState::Unconnected => {}
        }
        if inner.state == State::Listen {
            return Err(Errno::EINVAL);
        }
        let mut remote = Addr::cast(raw).map_err(|_| Errno::EINVAL)?;
        let route = self.transports.route(&mut remote)?;
        if self.kind == Kind::SeqPacket && matches!(route, Route::Host(_)) {
            return Err(Errno::ESOCKTNOSUPPORT);
        }
        inner.remote = remote;
        if !inner.local.bound() {
            self.bind_locked(&mut inner, Addr::UNBOUND, false)?;
        }
        inner.state = State::SynSent;
        inner.err = None;
        let local = inner.local;
        let buffer = inner.buffer as usize;
        drop(inner);
        let link = match route {
            Route::Host(network) => network
                .vsock_connect(remote.cid, remote.port, self.wait.clone())
                .map(Link::Host),
            Route::Local => self.connect_local(remote, local, buffer),
        };
        let mut inner = self.lock();
        match link {
            Ok(Link::Host(stream)) => {
                inner.link = Some(Link::Host(stream));
                inner.sock_state = SockState::Connecting;
                Self::refresh(&mut inner);
                Ok(inner.state == State::Established)
            }
            Ok(link) => {
                inner.link = Some(link);
                inner.state = State::Established;
                inner.sock_state = SockState::Connected;
                // The loopback transport answers from a worker: a non-blocking connect is
                // still in progress when it returns.
                Ok(!nonblocking)
            }
            // The reset arrives after the request went out: a non-blocking connect learns of
            // it through `sk_err`.
            Err(err) => {
                inner.state = State::Close;
                inner.sock_state = SockState::Unconnected;
                if !nonblocking {
                    return Err(err);
                }
                inner.err = Some(err);
                drop(inner);
                self.wait.wake(PollMask::empty());
                Ok(false)
            }
        }
    }

    /// A blocking connect's wait is over: connected, or failed with the socket's error.
    pub(crate) fn finish_connect(&self) -> Result<(), Errno> {
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        match inner.state {
            State::SynSent => Err(Errno::EAGAIN),
            State::Established => Ok(()),
            _ => Err(inner.err.take().unwrap_or(Errno::ECONNRESET)),
        }
    }

    /// The wait ran out (`ETIMEDOUT`) or was interrupted: the socket is unconnected again,
    /// keeping its binding.
    pub(crate) fn abort_connect(&self) {
        let mut inner = self.lock();
        if inner.state == State::SynSent {
            inner.state = State::Close;
            inner.sock_state = SockState::Unconnected;
            inner.link = None;
        }
    }

    pub(crate) fn connect_timeout(&self) -> Duration {
        self.lock().connect_timeout
    }

    /// The loopback transport's `VIRTIO_VSOCK_OP_REQUEST` at the listener
    /// (`virtio_transport_recv_listen`): a listener of the type bound to the address, with
    /// room in its queue (`sk_acceptq_is_full`), gets a connected child; anything else is a
    /// reset, which the connecting socket sees as `ECONNRESET`.
    fn connect_local(&self, remote: Addr, local: Addr, buffer: usize) -> Result<Link, Errno> {
        let listener = self.table.find(&remote).ok_or(Errno::ECONNRESET)?;
        let mut l = listener.lock();
        if l.state != State::Listen
            || listener.kind != self.kind
            || l.shutdown == SHUTDOWN_MASK
            || l.accept.len() as u32 > l.backlog
        {
            return Err(Errno::ECONNRESET);
        }
        let child = VsockSock::new(
            self.kind,
            self.table.clone(),
            listener.transports.clone(),
            listener.net.clone(),
            self.host.clone(),
            Some(&l),
        );
        let pipe = Pipe::new(
            [self.wait.clone(), child.wait.clone()],
            [buffer, l.buffer as usize],
        );
        {
            let mut c = child.lock();
            c.local = Addr::new(remote.cid, remote.port);
            // The loopback transport's own CID is the source of what it carries.
            c.remote = Addr::new(CID_LOCAL, local.port);
            c.state = State::Established;
            c.link = Some(Link::Local {
                pipe: pipe.clone(),
                side: 1,
            });
        }
        l.accept.push_back(child);
        drop(l);
        listener.wait.wake(PollMask::IN | PollMask::RDNORM);
        Ok(Link::Local { pipe, side: 0 })
    }

    /// `vsock_shutdown`.
    pub(crate) fn shutdown(&self, how: i32) -> Result<(), Errno> {
        let mode = (how + 1) as u8;
        if how < 0 || mode & !SHUTDOWN_MASK != 0 || mode == 0 {
            return Err(Errno::EINVAL);
        }
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        if inner.sock_state == SockState::Unconnected {
            return Err(Errno::ENOTCONN);
        }
        inner.sock_state = SockState::Disconnecting;
        inner.shutdown |= mode;
        match &inner.link {
            Some(Link::Local { pipe, side }) => pipe.shutdown(*side, mode),
            Some(Link::Host(stream)) => {
                let how = match mode {
                    RCV_SHUTDOWN => Shutdown::Read,
                    SEND_SHUTDOWN => Shutdown::Write,
                    _ => Shutdown::Both,
                };
                stream.shutdown(how)?;
            }
            None => {}
        }
        drop(inner);
        self.wait.wake(PollMask::empty());
        Ok(())
    }

    /// One attempt of `vsock_connectible_sendmsg`, `EAGAIN` while the peer has no room.
    pub(crate) fn try_send(&self, data: &[u8], flags: MsgFlags, to: bool) -> Result<usize, Errno> {
        if flags.contains(MsgFlags::MSG_OOB) {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        if to {
            return Err(match inner.state {
                State::Established => Errno::EISCONN,
                _ => Errno::EOPNOTSUPP,
            });
        }
        if inner.shutdown & SEND_SHUTDOWN != 0 || Self::peer_shutdown(&inner) & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if inner.state != State::Established || !inner.local.bound() {
            return Err(Errno::ENOTCONN);
        }
        if let Some(err) = inner.err.take() {
            return Err(err);
        }
        match &inner.link {
            Some(Link::Local { pipe, side }) => {
                pipe.write(*side, data, self.kind == Kind::SeqPacket)
            }
            Some(Link::Host(stream)) => stream.write(data),
            None => Err(Errno::ENOTCONN),
        }
    }

    /// The most a seqpacket send may carry: the peer's buffer, plus a byte so a larger
    /// message is still seen as one.
    pub(crate) fn max_record(&self) -> usize {
        match &self.lock().link {
            Some(Link::Local { pipe, side }) => pipe.peer_buf_alloc(*side).saturating_add(1),
            _ => BUFFER_SIZE as usize + 1,
        }
    }

    /// One attempt of `vsock_connectible_recvmsg` into `buf`: (bytes, the message's length,
    /// truncated), `EAGAIN` while nothing is there.
    pub(crate) fn try_recv(
        &self,
        buf: &mut [u8],
        flags: MsgFlags,
    ) -> Result<(usize, usize, bool), Errno> {
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        if inner.state != State::Established {
            // `SOCK_DONE` after an orderly end: 0, else not connected.
            return match inner.link.is_some() {
                true => Ok((0, 0, false)),
                false => Err(Errno::ENOTCONN),
            };
        }
        if flags.contains(MsgFlags::MSG_OOB) {
            return Err(Errno::EOPNOTSUPP);
        }
        if inner.shutdown & RCV_SHUTDOWN != 0 || buf.is_empty() {
            return Ok((0, 0, false));
        }
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        // `vsock_connectible_wait_data`.
        if self.has_data(&inner) == 0 {
            if let Some(err) = inner.err.take() {
                return Err(err);
            }
            if Self::peer_shutdown(&inner) & SEND_SHUTDOWN != 0 {
                return Ok((0, 0, false));
            }
            return Err(Errno::EAGAIN);
        }
        match &inner.link {
            Some(Link::Local { pipe, side }) => match self.kind {
                Kind::Stream => {
                    let n = pipe.read_stream(*side, buf, peek);
                    Ok((n, n, false))
                }
                Kind::SeqPacket => {
                    let (n, len) = pipe.read_record(*side, buf, peek).unwrap_or((0, 0));
                    Ok((n, len, len > n))
                }
            },
            Some(Link::Host(stream)) => {
                let n = stream.read(buf, peek)?;
                Ok((n, n, false))
            }
            None => Err(Errno::ENOTCONN),
        }
    }

    /// `SIOCINQ`: `EINVAL` for a listener, else the bytes queued.
    fn input_len(&self) -> Result<usize, Errno> {
        let inner = self.lock();
        if inner.state == State::Listen {
            return Err(Errno::EINVAL);
        }
        match &inner.link {
            Some(Link::Local { pipe, side }) => Ok(pipe.bytes(*side)),
            Some(Link::Host(stream)) => Ok(stream.input_len()),
            None => Err(Errno::EOPNOTSUPP),
        }
    }

    /// `SIOCOUTQ`: what the peer has not taken.
    fn output_len(&self) -> Result<usize, Errno> {
        let inner = self.lock();
        if inner.state == State::Listen {
            return Err(Errno::EINVAL);
        }
        match &inner.link {
            Some(Link::Local { pipe, side }) => Ok(pipe.bytes(1 - side)),
            Some(Link::Host(stream)) => Ok(stream.output_len()),
            None => Err(Errno::EOPNOTSUPP),
        }
    }

    /// `vsock_release`: shuts the connection down, resets the clients still queued on a
    /// listener, and gives the port back.
    fn release(&self) {
        let mut inner = self.lock();
        inner.shutdown = SHUTDOWN_MASK;
        if let Some(Link::Local { pipe, side }) = inner.link.take() {
            pipe.close(side);
        }
        let pending = std::mem::take(&mut inner.accept);
        let local = inner.local;
        drop(inner);
        for child in pending {
            child.release();
        }
        if local.bound() {
            self.table.unbind(&local, self);
        }
    }

    /// `vsock_update_buffer_size`.
    fn update_buffer(inner: &mut Inner, val: u64) {
        let val = val.max(inner.buffer_min).min(inner.buffer_max);
        if val != inner.buffer
            && let Some(Link::Local { pipe, side }) = &inner.link
        {
            pipe.set_buf_alloc(*side, val as usize);
        }
        inner.buffer = val;
    }

    /// `vsock_connectible_setsockopt` at level `AF_VSOCK`.
    pub(crate) fn set_vsock_option(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
        let u64_in = || -> Result<u64, Errno> {
            let raw = value.get(..8).ok_or(Errno::EINVAL)?;
            Ok(u64::from_le_bytes(raw.try_into().unwrap()))
        };
        let mut inner = self.lock();
        match VsockOption(name) {
            VsockOption::SO_VM_SOCKETS_BUFFER_SIZE => Self::update_buffer(&mut inner, u64_in()?),
            VsockOption::SO_VM_SOCKETS_BUFFER_MAX_SIZE => {
                inner.buffer_max = u64_in()?;
                let size = inner.buffer;
                Self::update_buffer(&mut inner, size);
            }
            VsockOption::SO_VM_SOCKETS_BUFFER_MIN_SIZE => {
                inner.buffer_min = u64_in()?;
                let size = inner.buffer;
                Self::update_buffer(&mut inner, size);
            }
            VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW
            | VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD => {
                let old = VsockOption(name) == VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD;
                let (sec, usec) = timeval_in(value, old)?;
                if sec < 0 || !(0..1_000_000).contains(&usec) || sec >= MAX_TIMEOUT_SECS {
                    return Err(Errno::ERANGE);
                }
                let timeout = Duration::from_secs(sec as u64) + Duration::from_micros(usec as u64);
                inner.connect_timeout = match timeout.is_zero() {
                    true => CONNECT_TIMEOUT,
                    false => timeout,
                };
            }
            _ => return Err(Errno::ENOPROTOOPT),
        }
        drop(inner);
        self.wait.wake(PollMask::WRITABLE);
        Ok(())
    }

    /// `vsock_connectible_getsockopt`.
    pub(crate) fn vsock_option(&self, name: i32) -> Result<Vec<u8>, Errno> {
        let inner = self.lock();
        let u64_out = |v: u64| Ok(v.to_le_bytes().to_vec());
        match VsockOption(name) {
            VsockOption::SO_VM_SOCKETS_BUFFER_SIZE => u64_out(inner.buffer),
            VsockOption::SO_VM_SOCKETS_BUFFER_MAX_SIZE => u64_out(inner.buffer_max),
            VsockOption::SO_VM_SOCKETS_BUFFER_MIN_SIZE => u64_out(inner.buffer_min),
            VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW
            | VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD => {
                let old = VsockOption(name) == VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD;
                Ok(timeval_out(inner.connect_timeout, old))
            }
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// `sock_setsockopt` for the socket's `SOL_SOCKET` options; `SO_RCVLOWAT` is
    /// `vsock_set_rcvlowat`'s, at most the buffer size.
    pub(crate) fn set_option(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
        let mut inner = self.lock();
        if SocketOption(name) == SocketOption::SO_RCVLOWAT {
            let val = opts::read_int(value)?;
            if i64::from(val) > inner.buffer as i64 {
                return Err(Errno::EINVAL);
            }
        }
        if matches!(
            SocketOption(name),
            SocketOption::SO_ZEROCOPY | SocketOption::SO_INQ
        ) {
            return Err(Errno::ENOPROTOOPT);
        }
        let bound = inner.local.bound();
        inner
            .opts
            .set(SocketLevel::SOL_SOCKET.0, name, value, bound)?;
        drop(inner);
        self.wait.wake(PollMask::empty());
        Ok(())
    }

    /// `sock_getsockopt` for this socket.
    pub(crate) fn getsockopt(&self, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        let mut inner = self.lock();
        Self::refresh(&mut inner);
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(self.socket_type().0),
            SocketOption::SO_PROTOCOL => int(0),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_VSOCK.0),
            SocketOption::SO_ERROR => int(inner.err.take().map_or(0, |e| e.0)),
            SocketOption::SO_ACCEPTCONN => int((inner.state == State::Listen) as i32),
            _ => {
                let size = match name == wasixv2_vnet::bpf::SO_GET_FILTER {
                    true => wasixv2_vnet::bpf::MAX_PROGRAM_LEN,
                    false => 64,
                };
                let mut out = vec![0; size];
                let n = inner.opts.get(SocketLevel::SOL_SOCKET.0, name, &mut out)?;
                out.truncate(n);
                Ok(out)
            }
        }
    }
}

/// A guest timeval (`sock_copy_user_timeval`): the old one's 32-bit seconds, the new one's
/// 64-bit.
fn timeval_in(value: &[u8], old: bool) -> Result<(i64, i64), Errno> {
    let word = |at: usize, len: usize| -> Result<i64, Errno> {
        let raw = value.get(at..at + len).ok_or(Errno::EINVAL)?;
        Ok(match len {
            4 => i64::from(i32::from_le_bytes(raw.try_into().unwrap())),
            _ => i64::from_le_bytes(raw.try_into().unwrap()),
        })
    };
    match old {
        true => Ok((word(0, 4)?, word(4, 4)?)),
        false => Ok((word(0, 8)?, word(8, 8)?)),
    }
}

/// `sock_get_timeout`.
fn timeval_out(timeout: Duration, old: bool) -> Vec<u8> {
    let (sec, usec) = (timeout.as_secs(), timeout.subsec_micros());
    match old {
        true => [(sec as i32).to_le_bytes(), (usec as i32).to_le_bytes()].concat(),
        false => [(sec as i64).to_le_bytes(), i64::from(usec).to_le_bytes()].concat(),
    }
}

impl Pollable for VsockFile {
    /// `vsock_poll` for connectible sockets.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        let sock = &self.0;
        poll_wait(&mut table, &sock.wait);
        let mut inner = sock.lock();
        VsockSock::refresh(&mut inner);
        let mut mask = PollMask::empty();
        if inner.err.is_some() {
            mask |= PollMask::ERR;
        }
        let peer_shutdown = VsockSock::peer_shutdown(&inner);
        // `vsock_poll_shutdown`.
        if inner.shutdown == SHUTDOWN_MASK
            || (inner.shutdown & SEND_SHUTDOWN != 0 && peer_shutdown & SEND_SHUTDOWN != 0)
        {
            mask |= PollMask::HUP;
        }
        if inner.shutdown & RCV_SHUTDOWN != 0 || peer_shutdown & SEND_SHUTDOWN != 0 {
            mask |= PollMask::RDHUP | PollMask::IN | PollMask::RDNORM;
        }
        if inner.state == State::Listen && !inner.accept.is_empty() {
            mask |= PollMask::IN | PollMask::RDNORM;
        }
        let established = inner.state == State::Established;
        if established && inner.shutdown & RCV_SHUTDOWN == 0 {
            let rcvlowat = inner.opts.rcvlowat.max(1);
            let ready = match &inner.link {
                Some(Link::Local { pipe, side }) => match sock.kind {
                    Kind::Stream => pipe.bytes(*side) >= rcvlowat,
                    Kind::SeqPacket => pipe.has_data(*side, true) > 0,
                },
                Some(Link::Host(stream)) => stream.input_len() >= rcvlowat,
                None => false,
            };
            if ready {
                mask |= PollMask::IN | PollMask::RDNORM;
            }
        }
        if established && inner.shutdown & SEND_SHUTDOWN == 0 {
            let room = match &inner.link {
                Some(Link::Local { pipe, side }) => pipe.space(*side) > 0,
                Some(Link::Host(stream)) => stream.poll(None).contains(PollMask::OUT),
                None => false,
            };
            if room {
                mask |= PollMask::OUT | PollMask::WRNORM;
            }
        }
        // `TCP_CLOSE`/`TCP_CLOSING`: as INET sockets, writable while not shut for sending.
        let closing = inner.state == State::Close || VsockSock::done(&inner);
        if closing && inner.shutdown & SEND_SHUTDOWN == 0 {
            mask |= PollMask::OUT | PollMask::WRNORM;
        }
        mask
    }
}

impl FileObject for VsockFile {
    fn messages(&self) -> bool {
        self.0.kind == Kind::SeqPacket
    }

    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.0.blocking(io, RCV, MsgFlags(0));
            let (n, _, _) = recv(&self.0, &mut wait, buf, MsgFlags(0)).await?;
            Ok(n)
        })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.0.blocking(io, SND, MsgFlags(0));
            send(&self.0, &mut wait, data, MsgFlags(0), false).await
        })
    }

    fn buffered(&self) -> usize {
        self.0.input_len().unwrap_or(0)
    }

    /// `vsock_do_ioctl`: `SIOCINQ` and `SIOCOUTQ`.
    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            let len = match request {
                IoctlRequest::FIONREAD => self.0.input_len()?,
                IoctlRequest::TIOCOUTQ => self.0.output_len()?,
                _ => return Err(Errno::ENOTTY.into()),
            };
            write_value(caller.memory(), GuestPtrMut::new(arg), &(len as i32)).await?;
            Ok(0)
        })
    }
}

/// One receive into `buf`, waiting unless non-blocking: (bytes, message length, truncated).
pub(crate) async fn recv(
    sock: &VsockSock,
    wait: &mut Blocking<'_>,
    buf: &mut [u8],
    flags: MsgFlags,
) -> SysResult<(usize, usize, bool)> {
    let flags = flags & !(MsgFlags::MSG_WAITALL | MsgFlags::MSG_DONTWAIT);
    wait.wait(WaitOn::Queue(&sock.wait), PollMask::READABLE, || {
        sock.try_recv(buf, flags)
    })
    .await
}

/// `vsock_connectible_sendmsg`: a blocking stream send goes on until everything is queued,
/// returning the partial count if it then fails; a message goes whole.
pub(crate) async fn send(
    sock: &VsockSock,
    wait: &mut Blocking<'_>,
    data: &[u8],
    flags: MsgFlags,
    to: bool,
) -> SysResult<usize> {
    let mut sent = 0;
    loop {
        let n = match wait
            .wait(WaitOn::Queue(&sock.wait), PollMask::WRITABLE, || {
                sock.try_send(&data[sent..], flags, to)
            })
            .await
        {
            Ok(n) => n,
            Err(_) if sent > 0 => return Ok(sent),
            Err(err) => return Err(err),
        };
        sent += n;
        if sock.kind != Kind::Stream || wait.nonblocking() || sent >= data.len() {
            return Ok(sent);
        }
    }
}

/// `/dev/vsock` (`vsock_device`): answers `IOCTL_VM_SOCKETS_GET_LOCAL_CID`.
pub(crate) struct VsockDevice {
    pub(crate) transports: Transports,
}

impl Pollable for VsockDevice {
    fn poll(&self, _: Option<&mut PollTable<'_>>) -> PollMask {
        PollMask::empty()
    }
}

impl FileObject for VsockDevice {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn can_poll(&self) -> bool {
        false
    }

    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            if request != IoctlRequest::IOCTL_VM_SOCKETS_GET_LOCAL_CID {
                return Err(Errno::ENOTTY.into());
            }
            let cid = self.transports.local_cid();
            write_value(caller.memory(), GuestPtrMut::new(arg), &cid).await?;
            Ok(0)
        })
    }
}

impl Kernel {
    /// The vsock transports of the machine: the g2h transport is the initial network's
    /// backend's, which every namespace reaches, as one virtio device serves a whole VM.
    pub(crate) fn vsock_transports(&self) -> Transports {
        Transports::new(self.init_ns.net.network.as_ref())
    }

    /// `vsock_create`: stream and seqpacket sockets; no datagram transport exists.
    pub(crate) fn vsock_socket(
        &self,
        caller: &Caller<'_>,
        kind: SocketType,
        protocol: i32,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        if protocol != 0 && protocol != AddressFamily::AF_VSOCK.0 {
            return Err(Errno::EPROTONOSUPPORT.into());
        }
        let kind = match kind {
            SocketType::SOCK_STREAM => Kind::Stream,
            SocketType::SOCK_SEQPACKET => Kind::SeqPacket,
            // `vsock_assign_transport` finds no `transport_dgram`.
            SocketType::SOCK_DGRAM => return Err(Errno::ENODEV.into()),
            _ => return Err(Errno::ESOCKTNOSUPPORT.into()),
        };
        let sock = VsockSock::new(
            kind,
            self.vsock.clone(),
            self.vsock_transports(),
            self.ns(caller).net.clone(),
            self.host.clone(),
            None,
        );
        self.install_vsock(caller, sock, flags)
    }

    pub(crate) fn install_vsock(
        &self,
        caller: &Caller<'_>,
        sock: Arc<VsockSock>,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        let file = self.vsock_file(caller, sock, flags);
        let [fd] = self.install_files(caller, [file], flags.contains(SocketFlags::SOCK_CLOEXEC))?;
        Ok(fd)
    }

    pub(crate) fn vsock_file(
        &self,
        caller: &Caller<'_>,
        sock: Arc<VsockSock>,
        flags: SocketFlags,
    ) -> OpenFile {
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let path = self.pseudo.socket(self.cred(caller).fs_owner());
        OpenFile::new(Backing::Stream(Arc::new(VsockFile(sock)), path), status)
    }

    /// `vsock_connect`: a blocking connect waits up to the socket's connect timeout
    /// (`ETIMEDOUT`), a non-blocking one reports `EINPROGRESS`.
    pub(crate) async fn vsock_connect(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &VsockSock,
        raw: &[u8],
    ) -> SysResult<i32> {
        let interrupt = self.interrupt(caller);
        let io = Self::io(caller, &*interrupt, file);
        let nonblocking = io.nonblocking();
        if sock.start_connect(raw, nonblocking)? {
            return Ok(0);
        }
        if nonblocking {
            return Err(Errno::EINPROGRESS.into());
        }
        let timeout = Some(sock.connect_timeout());
        let mut wait = Blocking::new(&*self.host, io, MsgFlags(0), timeout);
        let result = wait
            .wait(WaitOn::Queue(&sock.wait), PollMask::WRITABLE, || {
                sock.finish_connect()
            })
            .await;
        match result {
            Ok(()) => Ok(0),
            Err(SysErr::Errno(Errno::EAGAIN)) => {
                sock.abort_connect();
                Err(Errno::ETIMEDOUT.into())
            }
            Err(err @ (SysErr::Restart | SysErr::Errno(Errno::EINTR))) => {
                sock.abort_connect();
                Err(err)
            }
            Err(err) => Err(err),
        }
    }

    /// `vsock_bind`, with `CAP_NET_BIND_SERVICE` for the reserved ports.
    pub(crate) fn vsock_bind(
        &self,
        caller: &Caller<'_>,
        sock: &VsockSock,
        raw: &[u8],
    ) -> SysResult<i32> {
        let privileged = self.cred(caller).capable_init(Cap::NetBindService);
        sock.bind(raw, privileged)?;
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sock(kind: Kind, table: &Arc<Table>) -> Arc<VsockSock> {
        let net = crate::ns::Ns::initial(
            crate::ns::NsKind::Net,
            None,
            crate::ns::Net {
                network: None,
                unix_names: Arc::default(),
                sysctl: Default::default(),
                devconf: Default::default(),
            },
        );
        VsockSock::new(
            kind,
            table.clone(),
            Transports { g2h: None },
            net,
            Arc::new(crate::tests::TestHost),
            None,
        )
    }

    fn raw(cid: u32, port: u32) -> Vec<u8> {
        Addr::new(cid, port).encode()
    }

    /// A listener on `port` of any CID, and a client connected to it on `cid`, with the
    /// accepted socket.
    fn connected(kind: Kind, cid: u32) -> (Arc<VsockSock>, Arc<VsockSock>, Arc<VsockSock>) {
        let table = Arc::new(Table::default());
        let listener = sock(kind, &table);
        listener.bind(&raw(CID_ANY, 5000), false).unwrap();
        listener.listen(4).unwrap();
        let client = sock(kind, &table);
        assert_eq!(client.start_connect(&raw(cid, 5000), false), Ok(true));
        let child = listener.try_accept().unwrap();
        (listener, client, child)
    }

    #[test]
    fn loopback_streams_carry_bytes_both_ways_and_name_their_ends() {
        let (_l, client, child) = connected(Kind::Stream, CID_LOCAL);
        assert_eq!(client.try_send(b"hello", MsgFlags(0), false), Ok(5));
        let mut buf = [0; 3];
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((3, 3, false)));
        assert_eq!(&buf, b"hel");
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((2, 2, false)));
        assert_eq!(&buf[..2], b"lo");
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Err(Errno::EAGAIN));
        assert_eq!(child.try_send(b"x", MsgFlags(0), false), Ok(1));
        assert_eq!(client.try_recv(&mut buf, MsgFlags(0)), Ok((1, 1, false)));
        // The client autobound above the reserved ports with CID any; the child is the
        // listener's port at the CID dialled, its peer the loopback transport's CID.
        let local = Addr::cast(&client.local_addr()).unwrap();
        assert_eq!(local.cid, CID_ANY);
        assert!(local.port > LAST_RESERVED_PORT);
        assert_eq!(
            Addr::cast(&child.local_addr()).unwrap(),
            Addr::new(CID_LOCAL, 5000)
        );
        assert_eq!(
            Addr::cast(&child.peer_addr().unwrap()).unwrap(),
            Addr::new(CID_LOCAL, local.port)
        );
    }

    #[test]
    fn without_a_host_transport_the_host_cid_is_this_machine() {
        let (_l, client, _child) = connected(Kind::Stream, CID_HOST);
        assert_eq!(
            Addr::cast(&client.peer_addr().unwrap()).unwrap(),
            Addr::new(CID_HOST, 5000)
        );
        let table = Arc::new(Table::default());
        let other = sock(Kind::Stream, &table);
        assert_eq!(
            other.start_connect(&raw(3, 5000), false),
            Err(Errno::ENODEV)
        );
        assert_eq!(other.bind(&raw(3, 5000), false), Err(Errno::EADDRNOTAVAIL));
        assert_eq!(Transports { g2h: None }.local_cid(), CID_LOCAL);
    }

    #[test]
    fn refused_connections_are_resets() {
        let table = Arc::new(Table::default());
        let client = sock(Kind::Stream, &table);
        assert_eq!(
            client.start_connect(&raw(CID_LOCAL, 1234), false),
            Err(Errno::ECONNRESET)
        );
        // A non-blocking one reports it through `SO_ERROR`.
        assert_eq!(client.start_connect(&raw(CID_LOCAL, 1234), true), Ok(false));
        assert_eq!(
            client.getsockopt(SocketOption::SO_ERROR.0).unwrap(),
            Errno::ECONNRESET.0.to_le_bytes()
        );
        // A bound socket that does not listen, and a listener of the other type, reset too.
        let bound = sock(Kind::Stream, &table);
        bound.bind(&raw(CID_ANY, 2000), false).unwrap();
        assert_eq!(
            sock(Kind::Stream, &table).start_connect(&raw(CID_LOCAL, 2000), false),
            Err(Errno::ECONNRESET)
        );
        let seq = sock(Kind::SeqPacket, &table);
        seq.bind(&raw(CID_ANY, 2001), false).unwrap();
        seq.listen(1).unwrap();
        assert_eq!(
            sock(Kind::Stream, &table).start_connect(&raw(CID_LOCAL, 2001), false),
            Err(Errno::ECONNRESET)
        );
    }

    #[test]
    fn binding_follows_vsock_bind_connectible() {
        let table = Arc::new(Table::default());
        let a = sock(Kind::Stream, &table);
        assert_eq!(a.bind(&raw(CID_ANY, 80), false), Err(Errno::EACCES));
        a.bind(&raw(CID_LOCAL, 4000), false).unwrap();
        assert_eq!(a.bind(&raw(CID_LOCAL, 4001), false), Err(Errno::EINVAL));
        // Any CID collides with a specific one on the port.
        assert_eq!(
            sock(Kind::Stream, &table).bind(&raw(CID_ANY, 4000), false),
            Err(Errno::EADDRINUSE)
        );
        assert_eq!(
            sock(Kind::Stream, &table).bind(&raw(CID_LOCAL, 4000), true),
            Err(Errno::EADDRINUSE)
        );
        // A short address is EINVAL for bind, whatever the reason.
        assert_eq!(a.bind(&raw(CID_ANY, 1)[..8], false), Err(Errno::EINVAL));
        // Releasing gives the port back.
        a.release();
        sock(Kind::Stream, &table)
            .bind(&raw(CID_ANY, 4000), false)
            .unwrap();
        // Listening needs a bound, unconnected socket.
        assert_eq!(sock(Kind::Stream, &table).listen(1), Err(Errno::EINVAL));
    }

    #[test]
    fn a_full_accept_queue_resets_the_next_client() {
        let table = Arc::new(Table::default());
        let listener = sock(Kind::Stream, &table);
        listener.bind(&raw(CID_ANY, 6000), false).unwrap();
        listener.listen(1).unwrap();
        // `sk_acceptq_is_full` is "more than the backlog": two fit.
        let clients: Vec<_> = (0..3).map(|_| sock(Kind::Stream, &table)).collect();
        assert_eq!(
            clients[0].start_connect(&raw(CID_LOCAL, 6000), false),
            Ok(true)
        );
        assert_eq!(
            clients[1].start_connect(&raw(CID_LOCAL, 6000), false),
            Ok(true)
        );
        assert_eq!(
            clients[2].start_connect(&raw(CID_LOCAL, 6000), false),
            Err(Errno::ECONNRESET)
        );
        // Closing the listener ends the queued connections.
        listener.release();
        let mut buf = [0; 1];
        assert_eq!(
            clients[0].try_recv(&mut buf, MsgFlags(0)),
            Ok((0, 0, false))
        );
        assert_eq!(
            clients[0].try_send(b"x", MsgFlags(0), false),
            Err(Errno::EPIPE)
        );
    }

    #[test]
    fn seqpacket_keeps_messages_whole() {
        let (_l, client, child) = connected(Kind::SeqPacket, CID_LOCAL);
        assert_eq!(client.try_send(b"abcdef", MsgFlags(0), false), Ok(6));
        assert_eq!(client.try_send(b"gh", MsgFlags(0), false), Ok(2));
        let mut buf = [0; 4];
        assert_eq!(
            child.try_recv(&mut buf, MsgFlags::MSG_PEEK),
            Ok((4, 6, true))
        );
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((4, 6, true)));
        assert_eq!(&buf, b"abcd");
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((2, 2, false)));
        // Larger than the peer's whole buffer.
        let big = vec![0; BUFFER_SIZE as usize + 1];
        assert_eq!(
            client.try_send(&big, MsgFlags(0), false),
            Err(Errno::EMSGSIZE)
        );
    }

    #[test]
    fn credit_is_the_peers_buffer() {
        let (_l, client, child) = connected(Kind::Stream, CID_LOCAL);
        let size = 4096u64.to_le_bytes();
        child
            .set_vsock_option(VsockOption::SO_VM_SOCKETS_BUFFER_SIZE.0, &size)
            .unwrap();
        let data = vec![7; 10_000];
        assert_eq!(client.try_send(&data, MsgFlags(0), false), Ok(4096));
        assert_eq!(
            client.try_send(&data, MsgFlags(0), false),
            Err(Errno::EAGAIN)
        );
        let mut buf = vec![0; 1000];
        assert_eq!(
            child.try_recv(&mut buf, MsgFlags(0)),
            Ok((1000, 1000, false))
        );
        assert_eq!(client.try_send(&data, MsgFlags(0), false), Ok(1000));
        // The size is clamped to the minimum and maximum.
        child
            .set_vsock_option(
                VsockOption::SO_VM_SOCKETS_BUFFER_SIZE.0,
                &1u64.to_le_bytes(),
            )
            .unwrap();
        assert_eq!(
            child
                .vsock_option(VsockOption::SO_VM_SOCKETS_BUFFER_SIZE.0)
                .unwrap(),
            BUFFER_MIN.to_le_bytes()
        );
        assert_eq!(
            child.set_vsock_option(VsockOption::SO_VM_SOCKETS_BUFFER_SIZE.0, &[0; 4]),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn shutdown_and_close_reach_the_peer() {
        let (_l, client, child) = connected(Kind::Stream, CID_LOCAL);
        assert_eq!(client.try_send(b"ab", MsgFlags(0), false), Ok(2));
        client.shutdown(1).unwrap();
        assert_eq!(client.try_send(b"x", MsgFlags(0), false), Err(Errno::EPIPE));
        let file = VsockFile(child.clone());
        let mask = file.poll(None);
        assert!(mask.contains(PollMask::RDHUP | PollMask::IN));
        let mut buf = [0; 8];
        // Data first, then the end.
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((2, 2, false)));
        assert_eq!(child.try_recv(&mut buf, MsgFlags(0)), Ok((0, 0, false)));
        drop(file);
        // A shutdown of an unconnected socket.
        let table = Arc::new(Table::default());
        assert_eq!(sock(Kind::Stream, &table).shutdown(2), Err(Errno::ENOTCONN));
        assert_eq!(sock(Kind::Stream, &table).shutdown(3), Err(Errno::EINVAL));
    }

    #[test]
    fn connected_sockets_refuse_addresses_and_oob() {
        let (_l, client, _child) = connected(Kind::Stream, CID_LOCAL);
        assert_eq!(
            client.try_send(b"x", MsgFlags(0), true),
            Err(Errno::EISCONN)
        );
        assert_eq!(
            client.try_send(b"x", MsgFlags::MSG_OOB, false),
            Err(Errno::EOPNOTSUPP)
        );
        assert_eq!(
            client.start_connect(&raw(CID_LOCAL, 5000), false),
            Err(Errno::EISCONN)
        );
        let table = Arc::new(Table::default());
        let fresh = sock(Kind::Stream, &table);
        let mut buf = [0; 1];
        assert_eq!(fresh.try_recv(&mut buf, MsgFlags(0)), Err(Errno::ENOTCONN));
        assert_eq!(
            fresh.try_send(b"x", MsgFlags(0), false),
            Err(Errno::ENOTCONN)
        );
        assert_eq!(fresh.peer_addr(), Err(Errno::ENOTCONN));
    }

    /// A backend with a g2h transport that gives the machine CID 3 and refuses everything,
    /// noting where it was asked to connect.
    struct Guest(Mutex<Vec<(u32, u32)>>);

    impl Network for Guest {
        fn socket(
            &self,
            _: wasixv2_vnet::Domain,
            _: SocketKind,
            _: i32,
            _: u32,
        ) -> Result<Arc<dyn wasixv2_vnet::Socket>, Errno> {
            Err(Errno::EAFNOSUPPORT)
        }

        fn vsock_cid(&self) -> Option<u32> {
            Some(3)
        }

        fn vsock_connect(
            &self,
            cid: u32,
            port: u32,
            _: WaitQueue,
        ) -> Result<Arc<dyn BackendStream>, Errno> {
            self.0.lock_or_recover().push((cid, port));
            Err(Errno::ECONNRESET)
        }
    }

    #[test]
    fn a_guest_loops_its_own_cid_back_and_sends_the_rest_to_the_host() {
        let guest = Arc::new(Guest(Mutex::default()));
        let network: Arc<dyn Network> = guest.clone();
        let transports = Transports::new(Some(&network));
        assert_eq!(transports.local_cid(), 3);
        let table = Arc::new(Table::default());
        let make = || {
            let s = sock(Kind::Stream, &table);
            VsockSock::new(
                Kind::Stream,
                table.clone(),
                transports.clone(),
                s.net.clone(),
                s.host.clone(),
                None,
            )
        };
        let listener = make();
        listener.bind(&raw(3, 900), true).unwrap();
        listener.listen(1).unwrap();
        assert_eq!(make().start_connect(&raw(3, 900), false), Ok(true));
        // The host CID, and a sibling, which goes through the host with the flag set.
        let client = make();
        assert_eq!(
            client.start_connect(&raw(CID_HOST, 900), false),
            Err(Errno::ECONNRESET)
        );
        let sibling = make();
        assert_eq!(sibling.start_connect(&raw(4, 1), true), Ok(false));
        assert_eq!(sibling.lock().remote.flags, FLAG_TO_HOST);
        assert_eq!(*guest.0.lock_or_recover(), [(CID_HOST, 900), (4, 1)]);
        // Seqpacket has no host transport.
        let seq = VsockSock::new(
            Kind::SeqPacket,
            table.clone(),
            transports.clone(),
            listener.net.clone(),
            listener.host.clone(),
            None,
        );
        assert_eq!(
            seq.start_connect(&raw(CID_HOST, 1), false),
            Err(Errno::ESOCKTNOSUPPORT)
        );
        // Binding to the guest's own CID works, to the host's does not.
        assert_eq!(
            make().bind(&raw(CID_HOST, 901), false),
            Err(Errno::EADDRNOTAVAIL)
        );
    }

    #[test]
    fn the_connect_timeout_option_is_a_timeval() {
        let table = Arc::new(Table::default());
        let s = sock(Kind::Stream, &table);
        let name = VsockOption::SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW.0;
        let tv = [5i64.to_le_bytes(), 250_000i64.to_le_bytes()].concat();
        s.set_vsock_option(name, &tv).unwrap();
        assert_eq!(s.vsock_option(name).unwrap(), tv);
        let bad = [1i64.to_le_bytes(), 1_000_000i64.to_le_bytes()].concat();
        assert_eq!(s.set_vsock_option(name, &bad), Err(Errno::ERANGE));
        // Zero is the default.
        s.set_vsock_option(name, &[0; 16]).unwrap();
        assert_eq!(s.connect_timeout(), CONNECT_TIMEOUT);
        assert_eq!(s.set_vsock_option(99, &[0; 8]), Err(Errno::ENOPROTOOPT));
    }
}
