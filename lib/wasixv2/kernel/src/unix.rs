//! Unix domain sockets, as Linux's `net/unix/af_unix.c`: a kernel facility of their own, not
//! part of the network namespace behind vnet.
//!
//! - Stream, datagram and seqpacket sockets, unnamed (`socketpair`, unbound), in the abstract
//!   namespace, or bound to a filesystem path through a socket inode.
//! - Each socket owns its receive queue; senders push into the peer's queue. A connection is
//!   two sockets that name each other weakly, so closing either end is seen by the other.
//! - `SCM_RIGHTS` carries open files, `SCM_CREDENTIALS` the sender's pid, uid and gid.
//! - Records (datagrams, seqpackets) move into the receiver's queue and out to the reader
//!   without another copy.
//! - Files in flight are charged to the sending user, and sockets that only messages in flight
//!   keep alive are collected (`gc`).
//!
//! Not yet: `MSG_OOB`, `SO_PEEK_OFF`, and autobind on `SO_PASSCRED`.

mod gc;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use wasixv2_abi::{
    AddressFamily, GuestPtrMut, IoctlRequest, MsgFlags, SocketLevel, SocketOption, SocketType,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult, write_value};
use wasixv2_vfs::{Cap, FileType, LookupFlags, MAY_WRITE};
use wasixv2_vnet::{
    SocketKind,
    opts::{self, Changed, SockOpts},
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Kernel,
    file::{FileObject, Io, OpenFile},
    socket::{Blocking, RCV, SND, Timeouts, WaitOn},
};

use gc::{InFlight, Inflight};

/// `offsetof(struct sockaddr_un, sun_path)` and `sizeof(struct sockaddr_un)`.
const SUN_PATH: usize = 2;
const SOCKADDR_UN: usize = 110;
/// What `unix_dgram_sendmsg` keeps of `SO_SNDBUF` for the skb itself: a record may be at most
/// `sndbuf - 32` bytes.
const RECORD_OVERHEAD: usize = 32;
/// `net.unix.max_dgram_qlen`: a datagram queue longer than this is full.
const MAX_DGRAM_QLEN: usize = 10;
/// `SCM_MAX_FD`: the most descriptors one `SCM_RIGHTS` message carries.
const SCM_MAX_FD: usize = 253;
pub(crate) const SCM_RIGHTS: i32 = 1;
pub(crate) const SCM_CREDENTIALS: i32 = 2;

const RCV_SHUTDOWN: u8 = 1;
const SEND_SHUTDOWN: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Stream,
    Dgram,
    SeqPacket,
}

impl Kind {
    fn connection(self) -> bool {
        self != Kind::Dgram
    }

    fn socket_type(self) -> SocketType {
        match self {
            Kind::Stream => SocketType::SOCK_STREAM,
            Kind::Dgram => SocketType::SOCK_DGRAM,
            Kind::SeqPacket => SocketType::SOCK_SEQPACKET,
        }
    }
}

/// `struct ucred`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Ucred {
    pub(crate) pid: i32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl Ucred {
    pub(crate) fn to_bytes(self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(12);
        raw.extend(self.pid.to_le_bytes());
        raw.extend(self.uid.to_le_bytes());
        raw.extend(self.gid.to_le_bytes());
        raw
    }

    /// `cred_to_ucred` without a peer: pid 0 and the invalid ids.
    pub(crate) const NONE: Self = Self {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };

    fn from_bytes(raw: &[u8]) -> Option<Self> {
        let word = |i: usize| raw.get(i..i + 4).map(|b| b.try_into().unwrap());
        Some(Self {
            pid: i32::from_le_bytes(word(0)?),
            uid: u32::from_le_bytes(word(4)?),
            gid: u32::from_le_bytes(word(8)?),
        })
    }
}

/// What `SO_PEERCRED` and `SO_PEERGROUPS` report: the pid and credentials of the task that
/// made the connection, called `socketpair` or listened (`sk_peer_pid`, `sk_peer_cred`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct PeerCred {
    pub(crate) ucred: Ucred,
    /// The supplementary groups, ascending.
    pub(crate) groups: Arc<[u32]>,
}

/// Where a bound socket is found.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Key {
    /// An abstract name, without its leading NUL.
    Abstract(Vec<u8>),
    /// A socket inode: superblock id and inode number.
    Inode(u64, u64),
}

/// The bound Unix sockets of a network namespace (Linux's `unix_socket_table`), and the files
/// in flight between them.
// TODO: one per network namespace once namespaces exist.
#[derive(Default)]
pub(crate) struct Names {
    map: Mutex<HashMap<Key, Weak<Sock>>>,
    /// The next autobind name.
    next: Mutex<u32>,
    /// Files queued in `SCM_RIGHTS` messages, not yet received.
    pub(crate) inflight: Arc<Inflight>,
}

impl Names {
    fn find(&self, key: &Key) -> Option<Arc<Sock>> {
        self.map.lock().unwrap().get(key)?.upgrade()
    }

    /// Claims `key` for `sock`; `EADDRINUSE` if a live socket holds it.
    fn insert(&self, key: Key, sock: &Arc<Sock>) -> Result<(), Errno> {
        let mut map = self.map.lock().unwrap();
        if map.get(&key).is_some_and(|old| old.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        map.insert(key, Arc::downgrade(sock));
        Ok(())
    }

    fn remove(&self, key: &Key, sock: &Sock) {
        let mut map = self.map.lock().unwrap();
        if map
            .get(key)
            .is_some_and(|old| std::ptr::eq(old.as_ptr(), sock))
        {
            map.remove(key);
        }
    }
}

/// One queued message (Linux's skb on `sk_receive_queue`).
struct Msg {
    data: Vec<u8>,
    /// Bytes of `data` already read, for streams.
    read: usize,
    /// The sender's address, as `getsockname` would report it; `None` if unbound.
    from: Option<Vec<u8>>,
    rights: InFlight,
    cred: Ucred,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unconnected,
    Listening,
    Connected,
}

struct Inner {
    state: State,
    /// The bound address (family included), as `getsockname` reports it.
    addr: Option<Vec<u8>>,
    key: Option<Key>,
    peer: Option<Weak<Sock>>,
    rx: VecDeque<Msg>,
    rx_bytes: usize,
    backlog: usize,
    /// Connections waiting for `accept`: the server ends, already connected.
    accept: VecDeque<Arc<Sock>>,
    shutdown: u8,
    err: Option<Errno>,
    peer_cred: Option<PeerCred>,
    released: bool,
    /// The `SOL_SOCKET` options. `sk_sndbuf` bounds what this socket may have queued at its
    /// peer and a record's size; `SO_PASSCRED` asks for the sender's credentials.
    opts: SockOpts,
}

impl Inner {
    fn passcred(&self) -> bool {
        self.opts
            .flag(SocketLevel::SOL_SOCKET.0, SocketOption::SO_PASSCRED.0)
    }
}

/// A Unix socket (`struct unix_sock`).
pub(crate) struct Sock {
    kind: Kind,
    inner: Mutex<Inner>,
    /// Woken when data arrives, room appears at the peer, or the state changes.
    wait: WaitQueue,
    names: Arc<Names>,
    /// The credentials of the task that created the socket.
    owner: PeerCred,
    /// `SO_RCVTIMEO`/`SO_SNDTIMEO`.
    pub(crate) timeouts: Timeouts,
    host: Arc<dyn Host>,
    me: Weak<Sock>,
}

/// What a receive returns.
pub(crate) struct Received {
    /// The bytes received: at most what the caller asked for.
    pub(crate) data: Vec<u8>,
    /// A record's full length, which `MSG_TRUNC` makes `recv` return.
    pub(crate) full_len: usize,
    pub(crate) from: Option<Vec<u8>>,
    pub(crate) flags: MsgFlags,
    pub(crate) rights: Vec<Arc<OpenFile>>,
    pub(crate) cred: Option<Ucred>,
}

/// What a send carries besides the data.
#[derive(Default)]
pub(crate) struct Ancillary {
    pub(crate) rights: Vec<Arc<OpenFile>>,
    pub(crate) cred: Option<Ucred>,
    /// The sender's real uid, charged for the rights (`fpl->user`).
    pub(crate) uid: u32,
}

impl Sock {
    pub(crate) fn new(
        kind: Kind,
        names: Arc<Names>,
        owner: PeerCred,
        host: Arc<dyn Host>,
    ) -> Arc<Self> {
        let ty = match kind {
            Kind::Stream => SocketKind::Stream,
            Kind::Dgram | Kind::SeqPacket => SocketKind::Datagram,
        };
        Arc::new_cyclic(|me| Self {
            kind,
            inner: Mutex::new(Inner {
                state: State::Unconnected,
                addr: None,
                key: None,
                peer: None,
                rx: VecDeque::new(),
                rx_bytes: 0,
                backlog: 0,
                accept: VecDeque::new(),
                shutdown: 0,
                err: None,
                peer_cred: None,
                released: false,
                opts: SockOpts::unix(ty),
            }),
            wait: WaitQueue::new(),
            names,
            owner,
            timeouts: Timeouts::default(),
            host,
            me: me.clone(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    /// The queue woken whenever anything about the socket changes.
    pub(crate) fn queue(&self) -> &WaitQueue {
        &self.wait
    }

    /// The blocking of one call in direction `dir`.
    pub(crate) fn blocking<'a>(&'a self, io: Io<'a>, dir: usize, flags: MsgFlags) -> Blocking<'a> {
        Blocking::new(&*self.host, io, flags, self.timeouts.get(dir))
    }

    /// The largest record a send may carry (`sk_sndbuf - 32`); `None` for streams.
    pub(crate) fn max_record(&self) -> Option<usize> {
        match self.kind {
            Kind::Stream => None,
            _ => Some(self.lock().opts.sndbuf.saturating_sub(RECORD_OVERHEAD)),
        }
    }

    /// `unix_create_pair`'s linking of a `socketpair`.
    pub(crate) fn pair(a: &Arc<Sock>, b: &Arc<Sock>) {
        for (x, y) in [(a, b), (b, a)] {
            let mut inner = x.lock();
            inner.peer = Some(Arc::downgrade(y));
            inner.peer_cred = Some(y.owner.clone());
            if x.kind.connection() {
                inner.state = State::Connected;
            }
        }
    }

    fn peer(&self) -> Option<Arc<Sock>> {
        self.lock().peer.as_ref()?.upgrade()
    }

    pub(crate) fn local_addr(&self) -> Vec<u8> {
        self.lock().addr.clone().unwrap_or_else(unnamed)
    }

    /// `unix_getname(peer = 1)`.
    pub(crate) fn peer_addr(&self) -> Result<Vec<u8>, Errno> {
        let inner = self.lock();
        let connected = match self.kind.connection() {
            true => inner.state == State::Connected,
            false => inner.peer.is_some(),
        };
        if !connected {
            return Err(Errno::ENOTCONN);
        }
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        drop(inner);
        Ok(peer.map_or_else(unnamed, |peer| peer.local_addr()))
    }

    /// `unix_release_sock`: the last open file is gone. `embryo` for a connection nobody
    /// accepted, which resets its client whatever it had queued.
    fn release(&self, embryo: bool) {
        let (peer, accept, rx, key, unread) = {
            let mut inner = self.lock();
            if inner.released {
                return;
            }
            inner.released = true;
            inner.state = State::Unconnected;
            inner.shutdown = RCV_SHUTDOWN | SEND_SHUTDOWN;
            let unread = !inner.rx.is_empty();
            inner.rx_bytes = 0;
            (
                inner.peer.take().and_then(|peer| peer.upgrade()),
                std::mem::take(&mut inner.accept),
                std::mem::take(&mut inner.rx),
                inner.key.take(),
                unread,
            )
        };
        if let Some(key) = key {
            self.names.remove(&key, self);
        }
        if let Some(peer) = peer {
            if self.kind.connection() {
                let mut other = peer.lock();
                other.shutdown = RCV_SHUTDOWN | SEND_SHUTDOWN;
                if unread || embryo {
                    other.err = Some(Errno::ECONNRESET);
                }
            }
            peer.wait.wake(PollMask::empty());
        }
        // Connections nobody accepted go with the listener.
        for embryo in accept {
            embryo.release(true);
        }
        self.wait.wake(PollMask::empty());
        drop(rx);
        // `unix_schedule_gc(NULL)`: this may have left sockets only messages hold.
        self.names.inflight.released();
    }

    /// The open files of the Unix sockets in flight in this socket's queue, and for a listener
    /// in the queues of the connections it has not accepted (`scan_children`).
    fn inflight_sockets(&self) -> Vec<usize> {
        let (mut out, embryos) = {
            let inner = self.lock();
            (rights_keys(&inner.rx), inner.accept.clone())
        };
        for embryo in embryos {
            out.extend(rights_keys(&embryo.lock().rx));
        }
        out
    }

    /// `unix_collect_skb`: the garbage collector found this socket unreachable; its queue (and
    /// its embryos' queues) go to `hitlist`, to be dropped once no lock is held.
    fn purge(&self, hitlist: &mut Vec<Msg>) {
        let embryos = {
            let mut inner = self.lock();
            inner.rx_bytes = 0;
            hitlist.extend(inner.rx.drain(..));
            inner.accept.clone()
        };
        for embryo in embryos {
            let mut e = embryo.lock();
            e.rx_bytes = 0;
            hitlist.extend(e.rx.drain(..));
        }
    }

    /// `unix_bind` for a name already resolved to `key` and `addr`. The address is checked and
    /// claimed under the socket's lock (`bindlock`), so of two racing binds one fails.
    fn bind(&self, key: Key, addr: Vec<u8>) -> Result<(), Errno> {
        let me = self.me.upgrade().ok_or(Errno::EINVAL)?;
        let mut inner = self.lock();
        if inner.addr.is_some() {
            return Err(Errno::EINVAL);
        }
        self.names.insert(key.clone(), &me)?;
        inner.addr = Some(addr);
        inner.key = Some(key);
        Ok(())
    }

    fn is_bound(&self) -> bool {
        self.lock().addr.is_some()
    }

    /// `unix_autobind`: an abstract name of five hex digits.
    fn autobind(&self) -> Result<(), Errno> {
        if self.is_bound() {
            return Ok(());
        }
        for _ in 0..0x100000 {
            let n = {
                let mut next = self.names.next.lock().unwrap();
                *next = (*next + 1) & 0xfffff;
                *next
            };
            let name = format!("{n:05x}").into_bytes();
            let mut addr = unnamed();
            addr.push(0);
            addr.extend(&name);
            match self.bind(Key::Abstract(name), addr) {
                Err(Errno::EADDRINUSE) => continue,
                // Bound meanwhile by another thread: that name stands.
                Err(Errno::EINVAL) => return Ok(()),
                result => return result,
            }
        }
        Err(Errno::ENOSPC)
    }

    /// `unix_listen`. `cred` is the caller's, which `SO_PEERCRED` then reports and connecting
    /// clients get as their peer's (`update_peercred`).
    pub(crate) fn listen(&self, backlog: u32, cred: PeerCred) -> Result<(), Errno> {
        if !self.kind.connection() {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut inner = self.lock();
        if inner.addr.is_none() {
            return Err(Errno::EINVAL);
        }
        if !matches!(inner.state, State::Unconnected | State::Listening) {
            return Err(Errno::EINVAL);
        }
        inner.state = State::Listening;
        inner.backlog = backlog as usize;
        inner.peer_cred = Some(cred);
        drop(inner);
        self.wait.wake(PollMask::empty());
        Ok(())
    }

    /// `unix_stream_connect` up to the wait for room: queues a new server end on `target`.
    /// The state is checked and changed under this socket's lock, held while the listener's is
    /// taken, so two threads connecting one socket cannot both queue a connection.
    fn try_connect(&self, target: &Arc<Sock>, cred: &PeerCred) -> Result<(), Errno> {
        if target.kind != self.kind {
            return Err(Errno::EPROTOTYPE);
        }
        let mut inner = self.lock();
        match inner.state {
            State::Connected => return Err(Errno::EISCONN),
            State::Listening => return Err(Errno::EINVAL),
            State::Unconnected => {}
        }
        if std::ptr::eq(Arc::as_ptr(target), self) {
            // Not listening (checked above), so nothing to connect to.
            return Err(Errno::ECONNREFUSED);
        }
        let mut listener = target.lock();
        if listener.released || listener.state != State::Listening {
            return Err(Errno::ECONNREFUSED);
        }
        if listener.accept.len() > listener.backlog {
            return Err(Errno::EAGAIN);
        }
        let embryo = Sock::new(
            self.kind,
            self.names.clone(),
            target.owner.clone(),
            target.host.clone(),
        );
        {
            let mut e = embryo.lock();
            e.state = State::Connected;
            e.addr = listener.addr.clone();
            e.peer = Some(self.me.clone());
            e.peer_cred = Some(cred.clone());
            // `sk_clone_lock` copies the listener's options.
            e.opts = listener.opts.inherit();
        }
        listener.accept.push_back(embryo.clone());
        // `copy_peercred`: the client's peer is whoever listened.
        let listener_cred = listener.peer_cred.clone();
        drop(listener);
        inner.state = State::Connected;
        inner.peer = Some(Arc::downgrade(&embryo));
        inner.peer_cred = listener_cred;
        drop(inner);
        target.wait.wake(PollMask::READABLE);
        Ok(())
    }

    /// `unix_accept` without waiting.
    pub(crate) fn try_accept(&self) -> Result<Arc<Sock>, Errno> {
        let mut inner = self.lock();
        if inner.state != State::Listening {
            return Err(Errno::EINVAL);
        }
        let embryo = inner.accept.pop_front().ok_or(Errno::EAGAIN)?;
        drop(inner);
        // A connecting client may be waiting for room.
        self.wait.wake(PollMask::WRITABLE);
        Ok(embryo)
    }

    /// `unix_dgram_connect`.
    fn connect_dgram(&self, target: Option<Arc<Sock>>) -> Result<(), Errno> {
        if let Some(target) = &target
            && target.kind != self.kind
        {
            return Err(Errno::EPROTOTYPE);
        }
        // `unix_dgram_connect` sets no peer credentials.
        self.lock().peer = target.as_ref().map(Arc::downgrade);
        Ok(())
    }

    /// Whether `target`'s queue can take more from this socket: `sock_alloc_send_pskb`'s
    /// `sk_wmem_alloc < sk_sndbuf` for connections (what this socket has queued there standing
    /// in for its write memory), `unix_recvq_full` for datagrams.
    fn room_at(&self, sndbuf: usize, target: &Inner) -> bool {
        match self.kind {
            Kind::Dgram => target.rx.len() <= MAX_DGRAM_QLEN,
            Kind::Stream | Kind::SeqPacket => target.rx_bytes < sndbuf,
        }
    }

    /// One attempt to queue data at the peer of a connected socket. Streams take what fits;
    /// the rest is the caller's to retry. A record moves out of `data` when queued.
    fn try_send_connected(
        &self,
        data: &mut Vec<u8>,
        anc: &mut Ancillary,
        cred: Ucred,
    ) -> Result<usize, Errno> {
        let (peer, shutdown, err, sndbuf, from) = {
            let mut inner = self.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            let err = inner.err.take();
            (
                inner.peer.clone(),
                inner.shutdown,
                err,
                inner.opts.sndbuf,
                inner.addr.clone(),
            )
        };
        if let Some(err) = err {
            return Err(err);
        }
        // `unix_seqpacket_sendmsg` goes through `unix_dgram_sendmsg` and its size limit.
        if self.kind == Kind::SeqPacket && data.len() > sndbuf.saturating_sub(RECORD_OVERHEAD) {
            return Err(Errno::EMSGSIZE);
        }
        if shutdown & SEND_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        // `unix_stream_sendmsg` sends nothing for no data, and the files go with it.
        if self.kind == Kind::Stream && data.is_empty() {
            return Ok(0);
        }
        let peer = peer.and_then(|p| p.upgrade()).ok_or(Errno::EPIPE)?;
        let mut target = peer.lock();
        if target.released || target.shutdown & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if !self.room_at(sndbuf, &target) {
            return Err(Errno::EAGAIN);
        }
        let (n, bytes) = match self.kind {
            Kind::Stream => {
                let n = data.len().min(sndbuf - target.rx_bytes);
                match n == data.len() {
                    true => (n, std::mem::take(data)),
                    false => (n, data[..n].to_vec()),
                }
            }
            _ => (data.len(), std::mem::take(data)),
        };
        target.rx_bytes += n;
        target.rx.push_back(Msg {
            data: bytes,
            read: 0,
            from,
            rights: InFlight::new(
                std::mem::take(&mut anc.rights),
                anc.uid,
                &self.names.inflight,
            ),
            cred: anc.cred.unwrap_or(cred),
        });
        drop(target);
        peer.wait.wake(PollMask::READABLE);
        Ok(n)
    }

    /// One attempt to queue a datagram at `target` (`unix_dgram_sendmsg`). The datagram moves
    /// out of `data` when queued.
    fn try_send_dgram(
        &self,
        target: &Arc<Sock>,
        data: &mut Vec<u8>,
        anc: &mut Ancillary,
        cred: Ucred,
    ) -> Result<usize, Errno> {
        let (from, sndbuf) = {
            let inner = self.lock();
            if inner.shutdown & SEND_SHUTDOWN != 0 {
                return Err(Errno::EPIPE);
            }
            (inner.addr.clone(), inner.opts.sndbuf)
        };
        if data.len() > sndbuf.saturating_sub(RECORD_OVERHEAD) {
            return Err(Errno::EMSGSIZE);
        }
        let mut other = target.lock();
        if other.released {
            drop(other);
            // A dead peer is forgotten (`unix_dgram_disconnected`).
            let mut inner = self.lock();
            if inner
                .peer
                .as_ref()
                .is_some_and(|p| std::ptr::eq(p.as_ptr(), Arc::as_ptr(target)))
            {
                inner.peer = None;
            }
            return Err(Errno::ECONNREFUSED);
        }
        // A socket connected elsewhere only hears from its peer.
        if let Some(peer) = &other.peer
            && !std::ptr::eq(peer.as_ptr(), self)
        {
            return Err(Errno::EPERM);
        }
        if other.shutdown & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if !self.room_at(sndbuf, &other) {
            return Err(Errno::EAGAIN);
        }
        let len = data.len();
        other.rx_bytes += len;
        other.rx.push_back(Msg {
            data: std::mem::take(data),
            read: 0,
            from,
            rights: InFlight::new(
                std::mem::take(&mut anc.rights),
                anc.uid,
                &self.names.inflight,
            ),
            cred: anc.cred.unwrap_or(cred),
        });
        drop(other);
        target.wait.wake(PollMask::READABLE);
        Ok(len)
    }

    /// One attempt to receive at most `max` bytes (`unix_stream_read_generic`,
    /// `__unix_dgram_recvmsg`).
    fn try_recv(
        &self,
        max: usize,
        flags: MsgFlags,
        writer: Option<Ucred>,
    ) -> Result<Received, Errno> {
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        let mut inner = self.lock();
        if self.kind == Kind::Stream && inner.state == State::Listening {
            return Err(Errno::EINVAL);
        }
        if inner.rx.is_empty() {
            if let Some(err) = inner.err.take() {
                return Err(err);
            }
            if inner.shutdown & RCV_SHUTDOWN != 0 {
                return Ok(Received::eof());
            }
            if self.kind == Kind::Stream && inner.state != State::Connected {
                return Err(Errno::EINVAL);
            }
            if self.kind == Kind::SeqPacket && inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            return Err(Errno::EAGAIN);
        }
        let out = match self.kind {
            Kind::Stream => stream_take(&mut inner, max, peek, writer),
            _ => record_take(&mut inner, max, peek),
        };
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        let passcred = inner.passcred();
        drop(inner);
        if peek {
            self.names.inflight.peeked(&out.rights);
        }
        if !peek {
            // Room at this socket: wake senders waiting on it or on their own queue.
            self.wait.wake(PollMask::WRITABLE);
            if let Some(peer) = peer {
                peer.wait.wake(PollMask::WRITABLE);
            }
        }
        let mut out = out;
        if !passcred {
            out.cred = None;
        }
        Ok(out)
    }

    fn input_len(&self) -> usize {
        let inner = self.lock();
        match self.kind {
            Kind::Stream => inner.rx_bytes,
            _ => inner.rx.front().map_or(0, |m| m.data.len()),
        }
    }

    pub(crate) fn peer_cred(&self) -> Option<PeerCred> {
        self.lock().peer_cred.clone()
    }

    pub(crate) fn take_error(&self) -> Option<Errno> {
        self.lock().err.take()
    }

    pub(crate) fn is_listening(&self) -> bool {
        self.lock().state == State::Listening
    }

    /// `unix_shutdown`: mirrored on the peer of a connection.
    pub(crate) fn shutdown(&self, mode: u8) {
        let peer = {
            let mut inner = self.lock();
            inner.shutdown |= mode;
            inner.peer.as_ref().and_then(Weak::upgrade)
        };
        if let Some(peer) = peer
            && self.kind.connection()
        {
            let mut mirrored = 0;
            if mode & RCV_SHUTDOWN != 0 {
                mirrored |= SEND_SHUTDOWN;
            }
            if mode & SEND_SHUTDOWN != 0 {
                mirrored |= RCV_SHUTDOWN;
            }
            peer.lock().shutdown |= mirrored;
            peer.wait.wake(PollMask::empty());
        }
        self.wait.wake(PollMask::empty());
    }

    /// `sk_setsockopt` for a Unix socket; the socket layer has taken the timeouts and the
    /// read-only options already. `net_admin` is `CAP_NET_ADMIN`, which the forcing buffer
    /// sizes need.
    pub(crate) fn set_option(&self, name: i32, value: &[u8], net_admin: bool) -> Result<(), Errno> {
        let level = SocketLevel::SOL_SOCKET.0;
        let mut inner = self.lock();
        let changed = match SocketOption(name) {
            // `SO_SNDBUFFORCE`/`SO_RCVBUFFORCE` skip the `wmem_max`/`rmem_max` clamp.
            SocketOption::SO_SNDBUFFORCE | SocketOption::SO_RCVBUFFORCE => {
                if !net_admin {
                    return Err(Errno::EPERM);
                }
                let val = opts::read_int(value)?.max(0) as usize;
                match SocketOption(name) {
                    SocketOption::SO_SNDBUFFORCE => {
                        inner.opts.sndbuf = val.saturating_mul(2).max(opts::SOCK_MIN_SNDBUF)
                    }
                    _ => inner.opts.rcvbuf = val.saturating_mul(2).max(opts::SOCK_MIN_RCVBUF),
                }
                Changed::Buffers
            }
            _ => {
                let bound = inner.addr.is_some();
                inner.opts.set(level, name, value, bound)?
            }
        };
        drop(inner);
        if changed == Changed::Buffers {
            // More room may be there now.
            self.wait.wake(PollMask::WRITABLE);
        }
        Ok(())
    }
}

impl Received {
    fn eof() -> Self {
        Self {
            data: Vec::new(),
            full_len: 0,
            from: None,
            flags: MsgFlags(0),
            rights: Vec::new(),
            cred: None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }
}

/// The open files of the Unix sockets in flight in `rx`, once per message holding each.
fn rights_keys(rx: &VecDeque<Msg>) -> Vec<usize> {
    rx.iter()
        .flat_map(|msg| &msg.rights.files)
        .filter(|f| f.downcast::<UnixFile>().is_some())
        .map(|f| Arc::as_ptr(f) as usize)
        .collect()
}

/// A stream read of at most `max` bytes from consecutive messages
/// (`unix_stream_read_generic`): it ends after a message that carried files, and with
/// `SO_PASSCRED` it never glues messages of different writers, the first of them being
/// `writer` when the read continues one (`MSG_WAITALL`). A whole unread message moves out
/// without a copy.
fn stream_take(inner: &mut Inner, max: usize, peek: bool, writer: Option<Ucred>) -> Received {
    let check_creds = inner.passcred();
    let mut writer = writer;
    let mut out = Received::eof();
    let mut index = 0;
    while out.data.len() < max && index < inner.rx.len() {
        let msg = &mut inner.rx[index];
        if check_creds {
            match writer {
                Some(w) if w != msg.cred => break,
                Some(_) => {}
                None => writer = Some(msg.cred),
            }
        }
        out.cred.get_or_insert(msg.cred);
        let had_rights = !msg.rights.is_empty();
        let avail = msg.data.len() - msg.read;
        let n = avail.min(max - out.data.len());
        if peek {
            out.data.extend(&msg.data[msg.read..msg.read + n]);
            out.rights.extend(msg.rights.files.iter().cloned());
            index += 1;
        } else {
            out.rights.append(&mut msg.rights.take());
            if out.data.is_empty() && msg.read == 0 && n == avail {
                out.data = std::mem::take(&mut msg.data);
            } else {
                out.data.extend(&msg.data[msg.read..msg.read + n]);
            }
            msg.read += n;
            inner.rx_bytes -= n;
            if msg.read == msg.data.len() || msg.data.is_empty() {
                inner.rx.remove(index);
            }
        }
        if had_rights {
            break;
        }
    }
    out.full_len = out.data.len();
    out
}

/// A datagram or seqpacket read: one whole message, truncated to `max`.
fn record_take(inner: &mut Inner, max: usize, peek: bool) -> Received {
    let (mut data, from, rights, cred) = if peek {
        let msg = inner.rx.front().expect("checked non-empty");
        let n = msg.data.len().min(max);
        let data = msg.data[..n].to_vec();
        (data, msg.from.clone(), msg.rights.files.clone(), msg.cred)
    } else {
        let mut msg = inner.rx.pop_front().expect("checked non-empty");
        inner.rx_bytes -= msg.data.len();
        let rights = msg.rights.take();
        (
            std::mem::take(&mut msg.data),
            msg.from.take(),
            rights,
            msg.cred,
        )
    };
    let full_len = match peek {
        true => inner.rx.front().map_or(0, |m| m.data.len()),
        false => data.len(),
    };
    data.truncate(max);
    let mut flags = MsgFlags(0);
    if data.len() < full_len {
        flags |= MsgFlags::MSG_TRUNC;
    }
    Received {
        data,
        full_len,
        from,
        flags,
        rights,
        cred: Some(cred),
    }
}

/// The address of an unnamed socket: just the family.
fn unnamed() -> Vec<u8> {
    (AddressFamily::AF_UNIX.0 as u16).to_le_bytes().to_vec()
}

/// A guest `sockaddr_un`, decoded (`unix_validate_addr`, `unix_mkname_bsd`).
pub(crate) enum Name {
    Abstract(Vec<u8>),
    Path(Vec<u8>),
}

impl Name {
    /// `None` for a bare family, which `bind` takes as a request to autobind.
    pub(crate) fn decode(raw: &[u8]) -> Result<Option<Self>, Errno> {
        if raw.len() < SUN_PATH || raw.len() > SOCKADDR_UN {
            return Err(Errno::EINVAL);
        }
        if u16::from_le_bytes([raw[0], raw[1]]) != AddressFamily::AF_UNIX.0 as u16 {
            return Err(Errno::EINVAL);
        }
        let path = &raw[SUN_PATH..];
        match path.first() {
            None => Ok(None),
            Some(0) => Ok(Some(Name::Abstract(path[1..].to_vec()))),
            Some(_) => {
                let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
                Ok(Some(Name::Path(path[..end].to_vec())))
            }
        }
    }

    /// The address `getsockname` reports for a socket bound to this name.
    fn addr(&self) -> Vec<u8> {
        let mut addr = unnamed();
        match self {
            Name::Abstract(name) => {
                addr.push(0);
                addr.extend(name);
            }
            Name::Path(path) => {
                addr.extend(path);
                addr.push(0);
            }
        }
        addr
    }
}

/// The open-file object of a Unix socket. Dropping it is the socket's last close.
pub(crate) struct UnixFile(pub(crate) Arc<Sock>);

impl Drop for UnixFile {
    fn drop(&mut self) {
        self.0.release(false);
    }
}

impl Pollable for UnixFile {
    /// `unix_poll` and `unix_dgram_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        let sock = &self.0;
        poll_wait(&mut table, &sock.wait);
        let peer = sock.peer();
        if let Some(peer) = &peer
            && sock.kind == Kind::Dgram
        {
            poll_wait(&mut table, &peer.wait);
        }
        let inner = sock.lock();
        let mut mask = PollMask::empty();
        if inner.err.is_some() {
            mask |= PollMask::ERR;
        }
        if inner.shutdown == RCV_SHUTDOWN | SEND_SHUTDOWN {
            mask |= PollMask::HUP;
        }
        if inner.shutdown & RCV_SHUTDOWN != 0 {
            mask |= PollMask::RDHUP | PollMask::READABLE;
        }
        if !inner.rx.is_empty() || !inner.accept.is_empty() {
            mask |= PollMask::READABLE;
        }
        if sock.kind.connection() && inner.state == State::Unconnected {
            mask |= PollMask::HUP;
        }
        let writable_state = match sock.kind {
            Kind::Dgram => true,
            _ => inner.state == State::Connected,
        };
        let listening = inner.state == State::Listening;
        let shutdown_send = inner.shutdown & SEND_SHUTDOWN != 0;
        let sndbuf = inner.opts.sndbuf;
        drop(inner);
        if listening {
            return mask;
        }
        let room = match &peer {
            Some(peer) => {
                let other = peer.lock();
                other.released || sock.room_at(sndbuf, &other)
            }
            None => sock.kind == Kind::Dgram,
        };
        if (writable_state && room) || shutdown_send {
            mask |= PollMask::WRITABLE | PollMask::WRBAND;
        }
        mask
    }
}

impl FileObject for UnixFile {
    fn messages(&self) -> bool {
        self.0.kind() != Kind::Stream
    }

    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.0.blocking(io, RCV, MsgFlags(0));
            let got = recv(&self.0, &mut wait, buf.len(), MsgFlags(0), None).await?;
            buf[..got.len()].copy_from_slice(&got.data);
            Ok(got.len())
        })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let owner = self.0.owner.ucred;
            let mut wait = self.0.blocking(io, SND, MsgFlags(0));
            let anc = Ancillary::default();
            send(&self.0, &mut wait, data.to_vec(), None, anc, owner).await
        })
    }

    fn buffered(&self) -> usize {
        self.0.input_len()
    }

    /// `sock_splice_read`.
    fn splice_read(&self) -> bool {
        true
    }

    /// `SIOCINQ` (`FIONREAD`) and `SIOCOUTQ` (`TIOCOUTQ`).
    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            let len = match request {
                IoctlRequest::FIONREAD => {
                    if self.0.is_listening() {
                        return Err(Errno::EINVAL.into());
                    }
                    self.0.input_len()
                }
                IoctlRequest::TIOCOUTQ => match self.0.peer() {
                    Some(peer) => peer.lock().rx_bytes,
                    None => 0,
                },
                _ => return Err(Errno::ENOTTY.into()),
            };
            write_value(caller.memory(), GuestPtrMut::new(arg), &(len as i32)).await?;
            Ok(0)
        })
    }
}

/// One receive of at most `max` bytes, waiting unless non-blocking. `MSG_WAITALL` is the
/// caller's to loop on, passing as `writer` the credentials the read so far carried: with
/// `SO_PASSCRED` a stream read stops (with nothing) at another writer's data.
pub(crate) async fn recv(
    sock: &Sock,
    wait: &mut Blocking<'_>,
    max: usize,
    flags: MsgFlags,
    writer: Option<Ucred>,
) -> SysResult<Received> {
    wait.wait(WaitOn::Queue(&sock.wait), PollMask::READABLE, || {
        sock.try_recv(max, flags, writer)
    })
    .await
}

/// Sends `data`, to `to` for an unconnected datagram socket. A blocking stream send goes on
/// until everything is queued, returning the partial count if it then fails.
pub(crate) async fn send(
    sock: &Sock,
    wait: &mut Blocking<'_>,
    data: Vec<u8>,
    to: Option<Arc<Sock>>,
    mut anc: Ancillary,
    cred: Ucred,
) -> SysResult<usize> {
    let mut data = data;
    if sock.kind == Kind::Dgram {
        let target = match to {
            Some(target) => target,
            None => {
                let inner = sock.lock();
                match &inner.peer {
                    Some(peer) => peer.upgrade().ok_or(Errno::ECONNREFUSED)?,
                    None => return Err(Errno::ENOTCONN.into()),
                }
            }
        };
        let queue = &target.wait;
        return wait
            .wait(WaitOn::Queue(queue), PollMask::WRITABLE, || {
                sock.try_send_dgram(&target, &mut data, &mut anc, cred)
            })
            .await;
    }
    let total = data.len();
    let mut sent = 0;
    loop {
        let n = match wait
            .wait(WaitOn::Queue(&sock.wait), PollMask::WRITABLE, || {
                sock.try_send_connected(&mut data, &mut anc, cred)
            })
            .await
        {
            Ok(n) => n,
            Err(_) if sent > 0 => return Ok(sent),
            Err(err) => return Err(err),
        };
        sent += n;
        if sock.kind != Kind::Stream || wait.nonblocking() || sent >= total {
            return Ok(sent);
        }
        // A part was queued as a copy; what is left stays.
        data.drain(..n);
    }
}

impl Kernel {
    /// The credentials `SCM_CREDENTIALS` and `SO_PEERCRED` report for the caller.
    pub(crate) fn ucred(&self, caller: &Caller<'_>) -> Ucred {
        let process = self.process(caller.task());
        let (uid, gid) = process.cred().ucred_ids();
        Ucred {
            pid: process.pid.0 as i32,
            uid,
            gid,
        }
    }

    /// The caller as a socket's peer: what `SO_PEERCRED` and `SO_PEERGROUPS` report
    /// (`prepare_peercred`).
    pub(crate) fn peer_cred(&self, caller: &Caller<'_>) -> PeerCred {
        PeerCred {
            ucred: self.ucred(caller),
            groups: self.process(caller.task()).cred().groups.as_slice().into(),
        }
    }

    /// `scm_check_creds`: a process may claim its own pid and any of its real, effective or
    /// saved ids, and others with `CAP_SYS_ADMIN`, `CAP_SETUID` and `CAP_SETGID`.
    fn may_claim(&self, caller: &Caller<'_>, claim: &Ucred) -> bool {
        let process = self.process(caller.task());
        let cred = process.cred();
        (claim.pid == process.pid.0 as i32 || cred.capable(Cap::SysAdmin))
            && (cred.user.holds(claim.uid) || cred.capable(Cap::Setuid))
            && (cred.group.holds(claim.gid) || cred.capable(Cap::Setgid))
    }

    /// `unix_create`.
    pub(crate) fn unix_socket(
        &self,
        caller: &Caller<'_>,
        kind: SocketType,
        protocol: i32,
    ) -> Result<Arc<Sock>, Errno> {
        if protocol != 0 && protocol != AddressFamily::AF_UNIX.0 {
            return Err(Errno::EPROTONOSUPPORT);
        }
        let kind = match kind {
            SocketType::SOCK_STREAM => Kind::Stream,
            // Linux turns raw Unix sockets into datagram ones.
            SocketType::SOCK_DGRAM | SocketType::SOCK_RAW => Kind::Dgram,
            SocketType::SOCK_SEQPACKET => Kind::SeqPacket,
            _ => return Err(Errno::ESOCKTNOSUPPORT),
        };
        Ok(Sock::new(
            kind,
            self.unix_names.clone(),
            self.peer_cred(caller),
            self.host.clone(),
        ))
    }

    /// Finds the socket a name refers to (`unix_find_other`).
    async fn unix_find(&self, caller: &Caller<'_>, name: &Name) -> SysResult<Arc<Sock>> {
        match name {
            Name::Abstract(name) => Ok(self
                .unix_names
                .find(&Key::Abstract(name.clone()))
                .ok_or(Errno::ECONNREFUSED)?),
            Name::Path(path) => {
                let env = self.env(caller);
                let start = self.start(caller, &env, wasixv2_syscalls::Fd(-100), path)?;
                let found = self
                    .vfs
                    .lookup(&env.cx(), &start, path, LookupFlags::follow())
                    .await?;
                self.vfs.permission(&env.cred, &found, MAY_WRITE).await?;
                if found.dentry.file_type() != FileType::Socket {
                    return Err(Errno::ECONNREFUSED.into());
                }
                let key = Key::Inode(found.dentry.sb().id, found.dentry.node().ino());
                let sock = self.unix_names.find(&key).ok_or(Errno::ECONNREFUSED)?;
                Ok(sock)
            }
        }
    }

    /// `unix_bind`: abstract names, filesystem paths (which create a socket inode) and
    /// autobind.
    pub(crate) async fn unix_bind(
        &self,
        caller: &Caller<'_>,
        sock: &Sock,
        raw: &[u8],
    ) -> SysResult<i32> {
        let Some(name) = Name::decode(raw)? else {
            if sock.is_bound() {
                return Err(Errno::EINVAL.into());
            }
            sock.autobind()?;
            return Ok(0);
        };
        if sock.is_bound() {
            return Err(Errno::EINVAL.into());
        }
        let Name::Path(path) = &name else {
            let Name::Abstract(abstract_name) = &name else {
                unreachable!("not a path")
            };
            sock.bind(Key::Abstract(abstract_name.clone()), name.addr())?;
            return Ok(0);
        };
        let env = self.env(caller);
        let start = self.start(caller, &env, wasixv2_syscalls::Fd(-100), path)?;
        let mode = 0o140000 | 0o777;
        match self
            .vfs
            .mknod(&env.cx(), &start, path, mode, Default::default())
            .await
        {
            Err(errno) if errno == Errno::EEXIST => return Err(Errno::EADDRINUSE.into()),
            result => result?,
        }
        let found = self
            .vfs
            .lookup(&env.cx(), &start, path, LookupFlags::default())
            .await?;
        let key = Key::Inode(found.dentry.sb().id, found.dentry.node().ino());
        if let Err(errno) = sock.bind(key, name.addr()) {
            // `unix_bind_bsd`: a socket bound meanwhile leaves no inode behind.
            let _ = self.vfs.unlink(&env.cx(), &start, path).await;
            return Err(errno.into());
        }
        Ok(0)
    }

    /// `unix_stream_connect` and `unix_dgram_connect`.
    pub(crate) async fn unix_connect(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &Sock,
        raw: &[u8],
    ) -> SysResult<i32> {
        if sock.kind == Kind::Dgram && crate::socket::family(raw)? == AddressFamily::AF_UNSPEC {
            sock.connect_dgram(None)?;
            return Ok(0);
        }
        let name = Name::decode(raw)?.ok_or(Errno::EINVAL)?;
        let target = self.unix_find(caller, &name).await?;
        if sock.kind == Kind::Dgram {
            sock.connect_dgram(Some(target))?;
            return Ok(0);
        }
        let cred = self.peer_cred(caller);
        let interrupt = self.interrupt(caller);
        let mut wait = sock.blocking(Self::io(caller, &*interrupt, file), SND, MsgFlags(0));
        wait.wait(WaitOn::Queue(&target.wait), PollMask::WRITABLE, || {
            sock.try_connect(&target, &cred)
        })
        .await?;
        Ok(0)
    }

    /// Resolves a send's destination: `EISCONN`/`EOPNOTSUPP` for connection sockets given
    /// one, as `unix_stream_sendmsg` and `unix_seqpacket_sendmsg` decide.
    pub(crate) async fn unix_destination(
        &self,
        caller: &Caller<'_>,
        sock: &Sock,
        raw: Option<&[u8]>,
    ) -> SysResult<Option<Arc<Sock>>> {
        let Some(raw) = raw else { return Ok(None) };
        if sock.kind.connection() {
            if sock.kind == Kind::SeqPacket {
                // `unix_seqpacket_sendmsg` ignores the address once connected.
                return match sock.lock().state {
                    State::Connected => Ok(None),
                    _ => Err(Errno::ENOTCONN.into()),
                };
            }
            return Err(match sock.lock().state {
                State::Connected => Errno::EISCONN,
                _ => Errno::EOPNOTSUPP,
            }
            .into());
        }
        let name = Name::decode(raw)?.ok_or(Errno::EINVAL)?;
        Ok(Some(self.unix_find(caller, &name).await?))
    }

    /// `__scm_send` for a Unix socket: `SCM_RIGHTS` and `SCM_CREDENTIALS`.
    pub(crate) fn unix_ancillary(
        &self,
        caller: &Caller<'_>,
        controls: &[wasixv2_vnet::Control],
    ) -> SysResult<Ancillary> {
        let mut anc = Ancillary {
            uid: self.process(caller.task()).cred().user.real,
            ..Ancillary::default()
        };
        let level = SocketLevel::SOL_SOCKET.0;
        for control in controls.iter().filter(|c| c.level == level) {
            match control.kind {
                SCM_RIGHTS => {
                    // `scm_fp_copy`: whole descriptors only; none is no error.
                    let count = control.data.len() / 4;
                    if count == 0 {
                        continue;
                    }
                    if anc.rights.len() + count > SCM_MAX_FD {
                        return Err(Errno::EINVAL.into());
                    }
                    if self.too_many_unix_fds(caller, anc.uid) {
                        return Err(Errno::ETOOMANYREFS.into());
                    }
                    for fd in control.data.chunks_exact(4) {
                        let fd = i32::from_le_bytes(fd.try_into().unwrap());
                        let file = self.file(caller.task(), wasixv2_syscalls::Fd(fd))?;
                        anc.rights.push(file);
                    }
                }
                SCM_CREDENTIALS => {
                    let cred = Ucred::from_bytes(&control.data)
                        .filter(|_| control.data.len() == 12)
                        .ok_or(Errno::EINVAL)?;
                    if !self.may_claim(caller, &cred) {
                        return Err(Errno::EPERM.into());
                    }
                    anc.cred = Some(cred);
                }
                _ => return Err(Errno::EINVAL.into()),
            }
        }
        // `unix_schedule_gc(user)`: a user with many files in flight sending Unix sockets
        // collects first.
        let inflight = &self.unix_names.inflight;
        if anc
            .rights
            .iter()
            .any(|f| f.downcast::<UnixFile>().is_some())
            && inflight.user(anc.uid) >= gc::SANE_USER
        {
            inflight.collect();
        }
        Ok(anc)
    }

    /// `too_many_unix_fds`: the sending user has more files in flight than the sender's
    /// `RLIMIT_NOFILE`, and is not exempt through `CAP_SYS_RESOURCE` or `CAP_SYS_ADMIN`.
    fn too_many_unix_fds(&self, caller: &Caller<'_>, uid: u32) -> bool {
        let limit = self.files(caller.task()).lock().unwrap().limit();
        if self.unix_names.inflight.user(uid) <= limit {
            return false;
        }
        let cred = self.process(caller.task()).cred();
        !cred.capable(Cap::SysResource) && !cred.capable(Cap::SysAdmin)
    }

    /// `sk_getsockopt` for a Unix socket, the timeouts aside: what describes the socket, its
    /// peer's credentials, and the options `setsockopt` keeps. Values are written whole; the
    /// caller truncates them to the guest's length.
    pub(crate) fn unix_getsockopt(&self, sock: &Sock, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(sock.kind.socket_type().0),
            SocketOption::SO_PROTOCOL => int(0),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_UNIX.0),
            SocketOption::SO_ERROR => int(sock.take_error().map_or(0, |e| e.0)),
            SocketOption::SO_ACCEPTCONN => int(sock.is_listening() as i32),
            // `cred_to_ucred`: without a peer, pid 0 and the invalid ids.
            SocketOption::SO_PEERCRED => Ok(sock
                .peer_cred()
                .map_or(Ucred::NONE, |peer| peer.ucred)
                .to_bytes()),
            // `SO_PEERGROUPS` is answered by the socket layer, which may need `ERANGE`.
            _ => {
                let mut out = [0u8; 64];
                let n = sock
                    .lock()
                    .opts
                    .get(SocketLevel::SOL_SOCKET.0, name, &mut out)?;
                Ok(out[..n].to_vec())
            }
        }
    }
}

/// `shutdown`'s `how` as `RCV_SHUTDOWN`/`SEND_SHUTDOWN` bits.
pub(crate) fn shutdown_mode(how: i32) -> Result<u8, Errno> {
    match how {
        0 => Ok(RCV_SHUTDOWN),
        1 => Ok(SEND_SHUTDOWN),
        2 => Ok(RCV_SHUTDOWN | SEND_SHUTDOWN),
        _ => Err(Errno::EINVAL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(kind: Kind) -> (Arc<Sock>, Arc<Sock>) {
        let names = Arc::new(Names::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let a = Sock::new(kind, names.clone(), PeerCred::default(), host.clone());
        let b = Sock::new(kind, names, PeerCred::default(), host);
        Sock::pair(&a, &b);
        (a, b)
    }

    fn cred(pid: i32) -> Ucred {
        Ucred {
            pid,
            uid: 1,
            gid: 1,
        }
    }

    #[test]
    fn stream_reads_never_glue_writers_with_so_passcred() {
        let (a, b) = pair(Kind::Stream);
        let send = |data: &[u8], pid| {
            let mut data = data.to_vec();
            let mut anc = Ancillary::default();
            a.try_send_connected(&mut data, &mut anc, cred(pid))
                .unwrap()
        };
        send(b"ab", 10);
        send(b"cd", 11);
        // Without SO_PASSCRED the writers' bytes run together.
        let got = b.try_recv(16, MsgFlags::MSG_PEEK, None).unwrap();
        assert_eq!(got.data, b"abcd");
        b.lock()
            .opts
            .set(1, SocketOption::SO_PASSCRED.0, &1i32.to_le_bytes(), false)
            .unwrap();
        let first = b.try_recv(16, MsgFlags(0), None).unwrap();
        assert_eq!(
            (first.data.as_slice(), first.cred),
            (&b"ab"[..], Some(cred(10)))
        );
        // `MSG_WAITALL` going on from the first writer stops at the second's data.
        let more = b.try_recv(16, MsgFlags(0), first.cred).unwrap();
        assert_eq!(more.len(), 0);
        let second = b.try_recv(16, MsgFlags(0), None).unwrap();
        assert_eq!(
            (second.data.as_slice(), second.cred),
            (&b"cd"[..], Some(cred(11)))
        );
    }

    #[test]
    fn zero_length_stream_send_queues_nothing() {
        let (a, b) = pair(Kind::Stream);
        let file = a.clone();
        let mut anc = Ancillary::default();
        assert_eq!(
            a.try_send_connected(&mut Vec::new(), &mut anc, Ucred::default()),
            Ok(0)
        );
        assert_eq!(b.try_recv(16, MsgFlags(0), None).err(), Some(Errno::EAGAIN));
        drop(file);
    }

    #[test]
    fn records_move_whole_and_truncate_with_the_real_length() {
        let (a, b) = pair(Kind::SeqPacket);
        let mut anc = Ancillary::default();
        let mut data = vec![7u8; 100_000];
        assert_eq!(
            a.try_send_connected(&mut data, &mut anc, Ucred::default()),
            Ok(100_000)
        );
        assert!(data.is_empty(), "the record moved into the queue");
        let peeked = b.try_recv(10, MsgFlags::MSG_PEEK, None).unwrap();
        assert_eq!((peeked.len(), peeked.full_len), (10, 100_000));
        assert!(peeked.flags.contains(MsgFlags::MSG_TRUNC));
        let got = b.try_recv(1 << 20, MsgFlags(0), None).unwrap();
        assert_eq!((got.len(), got.full_len), (100_000, 100_000));
    }

    #[test]
    fn oversized_datagram_is_emsgsize() {
        let (a, b) = pair(Kind::Dgram);
        let mut anc = Ancillary::default();
        // `wmem_default` less the skb's share.
        let limit = opts::UDP_BUF_DEFAULT - RECORD_OVERHEAD;
        let mut data = vec![0u8; limit + 1];
        let sent = a.try_send_dgram(&b, &mut data, &mut anc, Ucred::default());
        assert_eq!(sent, Err(Errno::EMSGSIZE));
        let mut data = vec![0u8; limit];
        assert_eq!(
            a.try_send_dgram(&b, &mut data, &mut anc, Ucred::default()),
            Ok(limit)
        );
    }

    #[test]
    fn closing_a_listener_resets_queued_clients() {
        let names = Arc::new(Names::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let listener = Sock::new(
            Kind::Stream,
            names.clone(),
            PeerCred::default(),
            host.clone(),
        );
        listener
            .bind(Key::Abstract(b"l".to_vec()), b"\x01\x00\x00l".to_vec())
            .unwrap();
        listener.listen(4, PeerCred::default()).unwrap();
        let client = Sock::new(Kind::Stream, names, PeerCred::default(), host);
        client.try_connect(&listener, &PeerCred::default()).unwrap();
        listener.release(false);
        assert_eq!(
            client.try_recv(4, MsgFlags(0), None).err(),
            Some(Errno::ECONNRESET)
        );
        assert_eq!(client.try_recv(4, MsgFlags(0), None).unwrap().len(), 0);
    }

    #[test]
    fn connecting_twice_queues_one_connection() {
        let names = Arc::new(Names::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let listener = Sock::new(
            Kind::Stream,
            names.clone(),
            PeerCred::default(),
            host.clone(),
        );
        listener
            .bind(Key::Abstract(b"m".to_vec()), b"\x01\x00\x00m".to_vec())
            .unwrap();
        listener.listen(4, PeerCred::default()).unwrap();
        let client = Sock::new(Kind::Stream, names, PeerCred::default(), host);
        assert_eq!(client.try_connect(&listener, &PeerCred::default()), Ok(()));
        assert_eq!(
            client.try_connect(&listener, &PeerCred::default()),
            Err(Errno::EISCONN)
        );
        assert!(listener.try_accept().is_ok());
        assert_eq!(listener.try_accept().err(), Some(Errno::EAGAIN));
    }
}
