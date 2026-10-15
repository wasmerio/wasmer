//! Unix domain sockets, as Linux's `net/unix/af_unix.c`: a kernel facility of their own, not
//! part of the network namespace behind vnet.
//!
//! - Stream, datagram and seqpacket sockets, unnamed (`socketpair`, unbound), in the abstract
//!   namespace, or bound to a filesystem path through a socket inode.
//! - Each socket owns its receive queue; senders push into the peer's queue. A connection is
//!   two sockets that name each other weakly, so closing either end is seen by the other.
//! - `SCM_RIGHTS` carries open files, `SCM_CREDENTIALS` the sender's pid, uid and gid.
//!
//! Not yet: `SO_RCVTIMEO`/`SO_SNDTIMEO`, `MSG_OOB`, `SO_PEEK_OFF`, garbage collection of
//! sockets in flight in their own queues, and autobind on `SO_PASSCRED`.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

use wasixv2_abi::{AddressFamily, GuestPtrMut, IoctlRequest, MsgFlags, SocketOption, SocketType};
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vfs::{FileType, LookupFlags, MAY_WRITE};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{FileObject, Io, OpenFile},
};

/// `offsetof(struct sockaddr_un, sun_path)` and `sizeof(struct sockaddr_un)`.
const SUN_PATH: usize = 2;
const SOCKADDR_UN: usize = 110;
/// What a sender may have queued at its peer before it waits: `SO_SNDBUF`'s default
/// (`sysctl_wmem_default`).
pub(crate) const SNDBUF: usize = 212_992;
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

    fn from_bytes(raw: &[u8]) -> Option<Self> {
        let word = |i: usize| raw.get(i..i + 4).map(|b| b.try_into().unwrap());
        Some(Self {
            pid: i32::from_le_bytes(word(0)?),
            uid: u32::from_le_bytes(word(4)?),
            gid: u32::from_le_bytes(word(8)?),
        })
    }
}

/// Where a bound socket is found.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Key {
    /// An abstract name, without its leading NUL.
    Abstract(Vec<u8>),
    /// A socket inode: superblock id and inode number.
    Inode(u64, u64),
}

/// The bound Unix sockets of a network namespace (Linux's `unix_socket_table`).
// TODO: one per network namespace once namespaces exist.
#[derive(Default)]
pub(crate) struct Names {
    map: Mutex<HashMap<Key, Weak<Sock>>>,
    /// The next autobind name.
    next: Mutex<u32>,
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
    rights: Vec<Arc<OpenFile>>,
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
    passcred: bool,
    peer_cred: Option<Ucred>,
    released: bool,
}

/// A Unix socket (`struct unix_sock`).
pub(crate) struct Sock {
    kind: Kind,
    inner: Mutex<Inner>,
    /// Woken when data arrives, room appears at the peer, or the state changes.
    wait: WaitQueue,
    names: Arc<Names>,
    /// The credentials of the task that created the socket (`sk_peer_cred` at `listen`).
    owner: Ucred,
    me: Weak<Sock>,
}

/// What a receive returns.
pub(crate) struct Received {
    pub(crate) len: usize,
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
}

impl Sock {
    pub(crate) fn new(kind: Kind, names: Arc<Names>, owner: Ucred) -> Arc<Self> {
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
                passcred: false,
                peer_cred: None,
                released: false,
            }),
            wait: WaitQueue::new(),
            names,
            owner,
            me: me.clone(),
        })
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    /// `unix_create_pair`'s linking of a `socketpair`.
    pub(crate) fn pair(a: &Arc<Sock>, b: &Arc<Sock>) {
        for (x, y) in [(a, b), (b, a)] {
            let mut inner = x.inner.lock().unwrap();
            inner.peer = Some(Arc::downgrade(y));
            inner.peer_cred = Some(y.owner);
            if x.kind.connection() {
                inner.state = State::Connected;
            }
        }
    }

    fn peer(&self) -> Option<Arc<Sock>> {
        self.inner.lock().unwrap().peer.as_ref()?.upgrade()
    }

    pub(crate) fn local_addr(&self) -> Vec<u8> {
        let inner = self.inner.lock().unwrap();
        inner.addr.clone().unwrap_or_else(unnamed)
    }

    /// `unix_getname(peer = 1)`.
    pub(crate) fn peer_addr(&self) -> Result<Vec<u8>, Errno> {
        let inner = self.inner.lock().unwrap();
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

    /// `unix_release_sock`: the last open file is gone.
    fn release(&self) {
        let (peer, accept, rx, key, unread) = {
            let mut inner = self.inner.lock().unwrap();
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
                let mut other = peer.inner.lock().unwrap();
                other.shutdown = RCV_SHUTDOWN | SEND_SHUTDOWN;
                if unread {
                    other.err = Some(Errno::ECONNRESET);
                }
            }
            peer.wait.wake(PollMask::empty());
        }
        // Connections nobody accepted go with the listener.
        for embryo in accept {
            embryo.release();
        }
        self.wait.wake(PollMask::empty());
        drop(rx);
    }

    /// `unix_bind` for a name already resolved to `key` and `addr`.
    fn bind(&self, key: Key, addr: Vec<u8>) -> Result<(), Errno> {
        let me = self.me.upgrade().ok_or(Errno::EINVAL)?;
        if self.inner.lock().unwrap().addr.is_some() {
            return Err(Errno::EINVAL);
        }
        self.names.insert(key.clone(), &me)?;
        let mut inner = self.inner.lock().unwrap();
        inner.addr = Some(addr);
        inner.key = Some(key);
        Ok(())
    }

    /// `unix_autobind`: an abstract name of five hex digits.
    fn autobind(&self) -> Result<(), Errno> {
        if self.inner.lock().unwrap().addr.is_some() {
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
                result => return result,
            }
        }
        Err(Errno::ENOSPC)
    }

    /// `unix_listen`.
    pub(crate) fn listen(&self, backlog: u32) -> Result<(), Errno> {
        if !self.kind.connection() {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.addr.is_none() {
            return Err(Errno::EINVAL);
        }
        if !matches!(inner.state, State::Unconnected | State::Listening) {
            return Err(Errno::EINVAL);
        }
        inner.state = State::Listening;
        inner.backlog = backlog as usize;
        drop(inner);
        self.wait.wake(PollMask::empty());
        Ok(())
    }

    /// `unix_stream_connect` up to the wait for room: queues a new server end on `target`.
    fn try_connect(&self, target: &Arc<Sock>, cred: Ucred) -> Result<(), Errno> {
        if target.kind != self.kind {
            return Err(Errno::EPROTOTYPE);
        }
        match self.inner.lock().unwrap().state {
            State::Connected => return Err(Errno::EISCONN),
            State::Listening => return Err(Errno::EINVAL),
            State::Unconnected => {}
        }
        let mut listener = target.inner.lock().unwrap();
        if listener.released || listener.state != State::Listening {
            return Err(Errno::ECONNREFUSED);
        }
        if listener.accept.len() > listener.backlog {
            return Err(Errno::EAGAIN);
        }
        let embryo = Sock::new(self.kind, self.names.clone(), target.owner);
        {
            let mut inner = embryo.inner.lock().unwrap();
            inner.state = State::Connected;
            inner.addr = listener.addr.clone();
            inner.peer = Some(self.me.clone());
            inner.peer_cred = Some(cred);
            inner.passcred = listener.passcred;
        }
        listener.accept.push_back(embryo.clone());
        drop(listener);
        let mut inner = self.inner.lock().unwrap();
        inner.state = State::Connected;
        inner.peer = Some(Arc::downgrade(&embryo));
        inner.peer_cred = Some(target.owner);
        drop(inner);
        target.wait.wake(PollMask::READABLE);
        Ok(())
    }

    /// `unix_accept` without waiting.
    fn try_accept(&self) -> Result<Arc<Sock>, Errno> {
        let mut inner = self.inner.lock().unwrap();
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
        let mut inner = self.inner.lock().unwrap();
        inner.peer = target.as_ref().map(Arc::downgrade);
        inner.peer_cred = target.as_ref().map(|t| t.owner);
        Ok(())
    }

    /// Whether `target`'s queue can take `len` more bytes from this socket.
    fn room_at(&self, target: &Inner, len: usize) -> bool {
        match self.kind {
            Kind::Dgram => target.rx.len() <= MAX_DGRAM_QLEN,
            Kind::Stream => target.rx_bytes < SNDBUF,
            Kind::SeqPacket => target.rx_bytes + len <= SNDBUF || target.rx.is_empty(),
        }
    }

    /// One attempt to queue data at the peer of a connected socket. Streams take what fits;
    /// the rest is the caller's to retry.
    fn try_send_connected(
        &self,
        data: &[u8],
        anc: &mut Ancillary,
        cred: Ucred,
    ) -> Result<usize, Errno> {
        let (peer, shutdown, err) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            let err = inner.err.take();
            (inner.peer.clone(), inner.shutdown, err)
        };
        if let Some(err) = err {
            return Err(err);
        }
        if shutdown & SEND_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        let peer = peer.and_then(|p| p.upgrade()).ok_or(Errno::EPIPE)?;
        let from = self.inner.lock().unwrap().addr.clone();
        let mut target = peer.inner.lock().unwrap();
        if target.released || target.shutdown & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if !self.room_at(&target, data.len()) {
            return Err(Errno::EAGAIN);
        }
        let n = match self.kind {
            Kind::Stream => data.len().min(SNDBUF - target.rx_bytes),
            _ => data.len(),
        };
        if n == 0 && self.kind == Kind::Stream && !data.is_empty() {
            return Err(Errno::EAGAIN);
        }
        target.rx_bytes += n;
        target.rx.push_back(Msg {
            data: data[..n].to_vec(),
            read: 0,
            from,
            rights: std::mem::take(&mut anc.rights),
            cred: anc.cred.unwrap_or(cred),
        });
        drop(target);
        peer.wait.wake(PollMask::READABLE);
        Ok(n)
    }

    /// One attempt to queue a datagram at `target` (`unix_dgram_sendmsg`).
    fn try_send_dgram(
        &self,
        target: &Arc<Sock>,
        data: &[u8],
        anc: &mut Ancillary,
        cred: Ucred,
    ) -> Result<usize, Errno> {
        let from = {
            let inner = self.inner.lock().unwrap();
            if inner.shutdown & SEND_SHUTDOWN != 0 {
                return Err(Errno::EPIPE);
            }
            inner.addr.clone()
        };
        let mut other = target.inner.lock().unwrap();
        if other.released {
            drop(other);
            // A dead peer is forgotten (`unix_dgram_disconnected`).
            let mut inner = self.inner.lock().unwrap();
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
        if data.len() > SNDBUF {
            return Err(Errno::EMSGSIZE);
        }
        if !self.room_at(&other, data.len()) {
            return Err(Errno::EAGAIN);
        }
        other.rx_bytes += data.len();
        other.rx.push_back(Msg {
            data: data.to_vec(),
            read: 0,
            from,
            rights: std::mem::take(&mut anc.rights),
            cred: anc.cred.unwrap_or(cred),
        });
        drop(other);
        target.wait.wake(PollMask::READABLE);
        Ok(data.len())
    }

    /// One attempt to receive (`unix_stream_read_generic`, `__unix_dgram_recvmsg`).
    fn try_recv(&self, buf: &mut [u8], flags: MsgFlags) -> Result<Received, Errno> {
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        let mut inner = self.inner.lock().unwrap();
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
            Kind::Stream => stream_take(&mut inner, buf, peek),
            _ => record_take(&mut inner, buf, peek),
        };
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        let passcred = inner.passcred;
        drop(inner);
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
        let inner = self.inner.lock().unwrap();
        match self.kind {
            Kind::Stream => inner.rx_bytes,
            _ => inner.rx.front().map_or(0, |m| m.data.len()),
        }
    }

    pub(crate) fn set_passcred(&self, on: bool) {
        self.inner.lock().unwrap().passcred = on;
    }

    pub(crate) fn passcred(&self) -> bool {
        self.inner.lock().unwrap().passcred
    }

    pub(crate) fn peer_cred(&self) -> Option<Ucred> {
        self.inner.lock().unwrap().peer_cred
    }

    pub(crate) fn take_error(&self) -> Option<Errno> {
        self.inner.lock().unwrap().err.take()
    }

    pub(crate) fn is_listening(&self) -> bool {
        self.inner.lock().unwrap().state == State::Listening
    }

    /// `unix_shutdown`: mirrored on the peer of a connection.
    pub(crate) fn shutdown(&self, mode: u8) {
        let peer = {
            let mut inner = self.inner.lock().unwrap();
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
            peer.inner.lock().unwrap().shutdown |= mirrored;
            peer.wait.wake(PollMask::empty());
        }
        self.wait.wake(PollMask::empty());
    }
}

impl Received {
    fn eof() -> Self {
        Self {
            len: 0,
            full_len: 0,
            from: None,
            flags: MsgFlags(0),
            rights: Vec::new(),
            cred: None,
        }
    }
}

/// A stream read: bytes from consecutive messages, stopping after one that carried files.
fn stream_take(inner: &mut Inner, buf: &mut [u8], peek: bool) -> Received {
    let mut out = Received::eof();
    let mut copied = 0;
    let mut index = 0;
    while copied < buf.len() && index < inner.rx.len() {
        let msg = &mut inner.rx[index];
        if copied > 0 && (!msg.rights.is_empty() || Some(msg.cred) != out.cred) {
            break;
        }
        out.cred.get_or_insert(msg.cred);
        let avail = &msg.data[msg.read..];
        let n = avail.len().min(buf.len() - copied);
        buf[copied..copied + n].copy_from_slice(&avail[..n]);
        copied += n;
        let had_rights = !msg.rights.is_empty();
        if peek {
            out.rights.extend(msg.rights.iter().cloned());
            index += 1;
        } else {
            out.rights.append(&mut msg.rights);
            msg.read += n;
            inner.rx_bytes -= n;
            if msg.read == msg.data.len() {
                inner.rx.remove(index);
            }
        }
        if had_rights {
            break;
        }
    }
    out.len = copied;
    out.full_len = copied;
    out
}

/// A datagram or seqpacket read: one whole message, truncated to `buf`.
fn record_take(inner: &mut Inner, buf: &mut [u8], peek: bool) -> Received {
    let msg = match peek {
        true => None,
        false => inner.rx.pop_front(),
    };
    let (data, from, rights, cred) = match &msg {
        Some(msg) => (&msg.data, msg.from.clone(), msg.rights.clone(), msg.cred),
        None => {
            let msg = inner.rx.front().expect("checked non-empty");
            (&msg.data, msg.from.clone(), msg.rights.clone(), msg.cred)
        }
    };
    let n = data.len().min(buf.len());
    buf[..n].copy_from_slice(&data[..n]);
    let mut flags = MsgFlags(0);
    if n < data.len() {
        flags |= MsgFlags::MSG_TRUNC;
    }
    let full_len = data.len();
    if !peek {
        inner.rx_bytes -= full_len;
    }
    Received {
        len: n,
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
        self.0.release();
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
        let inner = sock.inner.lock().unwrap();
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
        let shutdown_send = inner.shutdown & SEND_SHUTDOWN != 0;
        drop(inner);
        if inner_listening(sock) {
            return mask;
        }
        let room = match &peer {
            Some(peer) => {
                let other = peer.inner.lock().unwrap();
                other.released || sock.room_at(&other, 1)
            }
            None => sock.kind == Kind::Dgram,
        };
        if (writable_state && room) || shutdown_send {
            mask |= PollMask::WRITABLE | PollMask::WRBAND;
        }
        mask
    }
}

fn inner_listening(sock: &Sock) -> bool {
    sock.inner.lock().unwrap().state == State::Listening
}

impl FileObject for UnixFile {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(recv(&self.0, io, buf, MsgFlags(0)).await?.len) })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let owner = self.0.owner;
            send(&self.0, io, data, None, Ancillary::default(), owner).await
        })
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
                    Some(peer) => peer.inner.lock().unwrap().rx_bytes,
                    None => 0,
                },
                _ => return Err(Errno::ENOTTY.into()),
            };
            write_value(caller.memory(), GuestPtrMut::new(arg), &(len as i32)).await?;
            Ok(0)
        })
    }
}

/// `MSG_DONTWAIT` makes one call non-blocking.
fn dont_wait(io: Io<'_>, flags: MsgFlags) -> Io<'_> {
    let mut io = io;
    if flags.contains(MsgFlags::MSG_DONTWAIT) {
        io.flags |= wasixv2_abi::OpenFlags::O_NONBLOCK;
    }
    io
}

/// Retries `op` while it fails with `EAGAIN`, sleeping on `queue`, unless non-blocking.
async fn block<T>(
    io: Io<'_>,
    queue: &WaitQueue,
    mut op: impl FnMut() -> Result<T, Errno>,
) -> SysResult<T> {
    let step = || match op() {
        Err(Errno::EAGAIN) if !io.nonblocking() => None,
        result => Some(result),
    };
    let result = wait_event(queue, step, io.interrupt)
        .await
        .map_err(|_| SysErr::Restart)?;
    Ok(result?)
}

/// Receives into `buf`, waiting unless non-blocking; `MSG_WAITALL` on a stream waits until
/// `buf` is full, the stream ends or fails.
pub(crate) async fn recv(
    sock: &Sock,
    io: Io<'_>,
    buf: &mut [u8],
    flags: MsgFlags,
) -> SysResult<Received> {
    let io = dont_wait(io, flags);
    let mut out = block(io, &sock.wait, || sock.try_recv(buf, flags)).await?;
    let waitall = flags.contains(MsgFlags::MSG_WAITALL)
        && !flags.contains(MsgFlags::MSG_PEEK)
        && sock.kind == Kind::Stream;
    while waitall && out.len > 0 && out.len < buf.len() && out.rights.is_empty() {
        let rest = &mut buf[out.len..];
        match block(io, &sock.wait, || sock.try_recv(rest, flags)).await {
            Ok(more) if more.len > 0 => {
                out.len += more.len;
                out.full_len += more.full_len;
                out.rights = more.rights;
            }
            _ => break,
        }
    }
    Ok(out)
}

/// Sends `data`, to `to` for an unconnected datagram socket. A blocking stream send goes on
/// until everything is queued, returning the partial count if it then fails.
pub(crate) async fn send(
    sock: &Sock,
    io: Io<'_>,
    data: &[u8],
    to: Option<Arc<Sock>>,
    mut anc: Ancillary,
    cred: Ucred,
) -> SysResult<usize> {
    if sock.kind == Kind::Dgram {
        let target = match to {
            Some(target) => target,
            None => {
                let inner = sock.inner.lock().unwrap();
                match &inner.peer {
                    Some(peer) => peer.upgrade().ok_or(Errno::ECONNREFUSED)?,
                    None => return Err(Errno::ENOTCONN.into()),
                }
            }
        };
        let queue = &target.wait;
        return block(io, queue, || {
            sock.try_send_dgram(&target, data, &mut anc, cred)
        })
        .await;
    }
    let mut sent = 0;
    loop {
        let rest = &data[sent..];
        match block(io, &sock.wait, || {
            sock.try_send_connected(rest, &mut anc, cred)
        })
        .await
        {
            Ok(n) => sent += n,
            Err(_) if sent > 0 => return Ok(sent),
            // TODO: raise SIGPIPE for EPIPE without MSG_NOSIGNAL once signals exist.
            Err(err) => return Err(err),
        }
        if sock.kind != Kind::Stream || io.nonblocking() || sent >= data.len() {
            return Ok(sent);
        }
    }
}

impl Kernel {
    /// The credentials `SCM_CREDENTIALS` and `SO_PEERCRED` report for the caller.
    pub(crate) fn ucred(&self, caller: &Caller<'_>) -> Ucred {
        Ucred {
            pid: self.process(caller.task()).pid.0 as i32,
            uid: crate::ID as u32,
            gid: crate::ID as u32,
        }
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
        Ok(Sock::new(kind, self.unix_names.clone(), self.ucred(caller)))
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
            if sock.inner.lock().unwrap().addr.is_some() {
                return Err(Errno::EINVAL.into());
            }
            sock.autobind()?;
            return Ok(0);
        };
        if sock.inner.lock().unwrap().addr.is_some() {
            return Err(Errno::EINVAL.into());
        }
        let key = match &name {
            Name::Abstract(name) => Key::Abstract(name.clone()),
            Name::Path(path) => {
                let env = self.env(caller);
                let start = self.start(caller, &env, wasixv2_syscalls::Fd(-100), path)?;
                let mode = 0o140000 | 0o777;
                match self
                    .vfs
                    .mknod(&env.cx(), &start, path, mode, Default::default())
                    .await
                {
                    Err(errno) if errno == Errno::EEXIST => {
                        return Err(Errno::EADDRINUSE.into());
                    }
                    result => result?,
                }
                let found = self
                    .vfs
                    .lookup(&env.cx(), &start, path, LookupFlags::default())
                    .await?;
                Key::Inode(found.dentry.sb().id, found.dentry.node().ino())
            }
        };
        sock.bind(key, name.addr())?;
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
        let cred = self.ucred(caller);
        let io = self.unix_io(caller, file);
        block(io, &target.wait, || sock.try_connect(&target, cred)).await?;
        Ok(0)
    }

    /// `unix_accept`.
    pub(crate) async fn unix_accept(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &Sock,
    ) -> SysResult<Arc<Sock>> {
        if !sock.kind.connection() {
            return Err(Errno::EOPNOTSUPP.into());
        }
        let io = self.unix_io(caller, file);
        block(io, &sock.wait, || sock.try_accept()).await
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
                return match sock.inner.lock().unwrap().state {
                    State::Connected => Ok(None),
                    _ => Err(Errno::ENOTCONN.into()),
                };
            }
            return Err(match sock.inner.lock().unwrap().state {
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
        let mut anc = Ancillary::default();
        let level = wasixv2_abi::SocketLevel::SOL_SOCKET.0;
        for control in controls.iter().filter(|c| c.level == level) {
            match control.kind {
                SCM_RIGHTS => {
                    if control.data.len() % 4 != 0 {
                        return Err(Errno::EINVAL.into());
                    }
                    let count = control.data.len() / 4;
                    if count == 0 || anc.rights.len() + count > SCM_MAX_FD {
                        return Err(Errno::EINVAL.into());
                    }
                    for fd in control.data.chunks(4) {
                        let fd = i32::from_le_bytes(fd.try_into().unwrap());
                        let file = self.file(caller.task(), wasixv2_syscalls::Fd(fd))?;
                        anc.rights.push(file);
                    }
                }
                SCM_CREDENTIALS => {
                    let cred = Ucred::from_bytes(&control.data)
                        .filter(|_| control.data.len() == 12)
                        .ok_or(Errno::EINVAL)?;
                    // Without capabilities, a process can only claim itself.
                    if cred != self.ucred(caller) {
                        return Err(Errno::EPERM.into());
                    }
                    anc.cred = Some(cred);
                }
                _ => return Err(Errno::EINVAL.into()),
            }
        }
        Ok(anc)
    }

    /// The `SOL_SOCKET` options of a Unix socket.
    pub(crate) fn unix_getsockopt(&self, sock: &Sock, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(sock.kind.socket_type().0),
            SocketOption::SO_PROTOCOL => int(0),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_UNIX.0),
            SocketOption::SO_ERROR => int(sock.take_error().map_or(0, |e| e.0)),
            SocketOption::SO_ACCEPTCONN => int(sock.is_listening() as i32),
            SocketOption::SO_PASSCRED => int(sock.passcred() as i32),
            SocketOption::SO_PEERCRED => {
                // Unconnected sockets report an invalid pid and the overflow ids.
                let cred = sock.peer_cred().unwrap_or(Ucred {
                    pid: 0,
                    uid: 65534,
                    gid: 65534,
                });
                Ok(cred.to_bytes())
            }
            SocketOption::SO_SNDBUF | SocketOption::SO_RCVBUF => int(SNDBUF as i32),
            SocketOption::SO_REUSEADDR
            | SocketOption::SO_KEEPALIVE
            | SocketOption::SO_BROADCAST => int(0),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// The call context for an operation on `file`.
    pub(crate) fn unix_io<'a>(&'a self, caller: &Caller<'_>, file: &OpenFile) -> Io<'a> {
        Io {
            flags: file.flags(),
            interrupt: self.interrupt(caller),
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
