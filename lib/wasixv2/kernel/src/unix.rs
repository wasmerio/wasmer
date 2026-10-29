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
//! - `MSG_OOB` on stream sockets (`CONFIG_AF_UNIX_OOB`): the last byte of such a send is the
//!   receiver's out-of-band byte, which `recv(MSG_OOB)` takes, `SIOCATMARK` and `POLLPRI`
//!   report, and ordinary reads stop at and skip unless `SO_OOBINLINE`.
//! - `SO_PEEK_OFF`: `MSG_PEEK` reads from an offset that peeking advances and reading takes
//!   back.
//! - With `SO_PASSCRED` an unbound socket autobinds before it connects or sends.
//!
//! Not yet: `SIGURG` for the out-of-band byte (no `F_SETOWN` delivery).

mod gc;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use wasixv2_abi::{
    AddressFamily, GuestPtrMut, IoctlRequest, MsgFlags, SiCode, SocketLevel, SocketOption,
    SocketType,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult, write_value};
use wasixv2_vfs::{Cap, FileType, LookupFlags, MAY_WRITE};
use wasixv2_vnet::{
    SocketKind,
    opts::{Changed, SockOpts},
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Kernel,
    fasync::{Fasync, SockAsync, SockWake},
    file::{FileObject, Io, OpenFile, ReadSink, delivered, taken},
    ns::NetNs,
    pid::PidRef,
    socket::{Blocking, RCV, SND, Timeouts, WaitOn},
    sync::MutexExt,
    uidgid::{Kgid, Kuid},
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

/// The credentials a socket keeps of a process: its `struct pid`, held as Linux's
/// `sk_peer_pid` and `UNIXCB(skb).pid` hold it, so a reaped process still shows its number,
/// and kernel ids. Each reader sees its own pid and ids for them ([`Kernel::ucred_bytes`]).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Ucred {
    pub(crate) pid: Option<PidRef>,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl Ucred {
    /// `cred_to_ucred` without a peer: pid 0 and the invalid ids.
    pub(crate) const NONE: Self = Self {
        pid: None,
        uid: u32::MAX,
        gid: u32::MAX,
    };
}

/// `struct ucred` as the guest reads and writes it: a pid and ids of its namespaces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct RawUcred {
    pub(crate) pid: i32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl RawUcred {
    /// `cred_to_ucred` without a peer: pid 0 and the invalid ids.
    pub(crate) const NONE: Self = Self {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };

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

/// What `SO_PEERCRED` and `SO_PEERGROUPS` report: the pid and credentials of the task that
/// made the connection, called `socketpair` or listened (`sk_peer_pid`, `sk_peer_cred`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct PeerCred {
    pub(crate) ucred: Ucred,
    /// The supplementary groups, ascending.
    pub(crate) groups: Arc<[Kgid]>,
}

/// Where a bound socket is found.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Key {
    /// An abstract name, without its leading NUL.
    Abstract(Vec<u8>),
    /// A socket inode: superblock id and inode number.
    Inode(u64, u64),
}

/// Where bound Unix sockets are found: a network namespace's by abstract name (Linux's
/// `net->unx.table`), the machine's by socket inode (`bsd_socket_buckets`, which no namespace
/// separates: a path is reached through the mount tree), and the files in flight between all
/// of them (`unix_tot_inflight`, `user->unix_inflight`), which no namespace separates either.
/// The namespace also lists every socket made in it, bound or not, until it is released, as
/// its table hashes them all (`unix_insert_unbound_socket`); `/proc/net/unix` shows them.
#[derive(Default)]
pub(crate) struct Names {
    abstract_names: Table,
    /// The namespace's live sockets, by [`Sock::id`].
    sockets: Mutex<BTreeMap<u64, Weak<Sock>>>,
    /// The next autobind name.
    next: Mutex<u32>,
    paths: Arc<Table>,
    /// Files queued in `SCM_RIGHTS` messages, not yet received.
    pub(crate) inflight: Arc<Inflight>,
}

/// Bound sockets by key.
#[derive(Default)]
struct Table(Mutex<HashMap<Key, Weak<Sock>>>);

impl Names {
    /// The names of a new network namespace (`unix_net_init`): no abstract names yet, and the
    /// machine's sockets bound to paths and files in flight.
    pub(crate) fn for_new_net(&self) -> Self {
        Self {
            abstract_names: Table::default(),
            sockets: Mutex::default(),
            next: Mutex::default(),
            paths: self.paths.clone(),
            inflight: self.inflight.clone(),
        }
    }

    /// The namespace's live sockets, oldest first.
    pub(crate) fn sockets(&self) -> Vec<Arc<Sock>> {
        let sockets = self.sockets.lock_or_recover();
        sockets.values().filter_map(Weak::upgrade).collect()
    }

    fn table(&self, key: &Key) -> &Table {
        match key {
            Key::Abstract(_) => &self.abstract_names,
            Key::Inode(..) => &self.paths,
        }
    }

    fn find(&self, key: &Key) -> Option<Arc<Sock>> {
        self.table(key).0.lock_or_recover().get(key)?.upgrade()
    }

    /// Claims `key` for `sock`; `EADDRINUSE` if a live socket holds it.
    fn insert(&self, key: Key, sock: &Arc<Sock>) -> Result<(), Errno> {
        let mut map = self.table(&key).0.lock_or_recover();
        if map.get(&key).is_some_and(|old| old.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        map.insert(key, Arc::downgrade(sock));
        Ok(())
    }

    fn remove(&self, key: &Key, sock: &Sock) {
        let mut map = self.table(key).0.lock_or_recover();
        if map
            .get(key)
            .is_some_and(|old| std::ptr::eq(old.as_ptr(), sock))
        {
            map.remove(key);
        }
    }
}

/// Whether a message is the out-of-band byte of a stream.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Urg {
    /// Ordinary data.
    None,
    /// The out-of-band byte (`u->oob_skb`).
    Mark,
    /// An out-of-band byte `recv(MSG_OOB)` took: empty, it still marks where it was.
    Spent,
}

/// One queued message (Linux's skb on `sk_receive_queue`).
struct Msg {
    urg: Urg,
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
    /// For a datagram socket, `sk_state == TCP_ESTABLISHED`: it connected, or another
    /// socket connected to it, and has not disconnected since (`unix_dgram_connect`).
    established: bool,
    /// The inode of the socket's file once it has one (`sock_graft`; `sock_i_ino`).
    ino: u64,
    /// The socket inode a path name made, and its filesystem's device (`unix_diag_vfs`).
    vfs: Option<(u64, wasixv2_vfs::Device)>,
    /// The `SOL_SOCKET` options. `sk_sndbuf` bounds what this socket may have queued at its
    /// peer and a record's size; `SO_PASSCRED` asks for the sender's credentials.
    opts: SockOpts,
    /// `SO_PEEK_OFF` (`sk_peek_off`): where `MSG_PEEK` reads from, -1 for the start.
    peek_off: i32,
}

impl Inner {
    fn passcred(&self) -> bool {
        self.opts
            .flag(SocketLevel::SOL_SOCKET.0, SocketOption::SO_PASSCRED.0)
    }

    /// `SOCK_URGINLINE`.
    fn oob_inline(&self) -> bool {
        self.opts
            .flag(SocketLevel::SOL_SOCKET.0, SocketOption::SO_OOBINLINE.0)
    }

    /// `u->oob_skb`: whether an out-of-band byte is waiting.
    fn has_oob(&self) -> bool {
        self.rx.iter().any(|m| m.urg == Urg::Mark)
    }

    /// `sk_peek_offset_fwd` (`n` positive) and `sk_peek_offset_bwd` (negative).
    fn move_peek_off(&mut self, n: isize) {
        if self.peek_off >= 0 {
            self.peek_off = (self.peek_off as isize + n).clamp(0, i32::MAX as isize) as i32;
        }
    }
}

/// A Unix socket (`struct unix_sock`).
pub(crate) struct Sock {
    /// Orders the namespace's list of sockets; never reused.
    id: u64,
    kind: Kind,
    inner: Mutex<Inner>,
    /// Woken when data arrives, room appears at the peer, or the state changes.
    wait: WaitQueue,
    /// The files that get I/O signals.
    fasync: SockAsync,
    /// The network namespace the socket was made in (`sock_net`).
    net: Arc<NetNs>,
    /// The credentials of the task that created the socket.
    owner: PeerCred,
    /// Its creator's Landlock domain (that of the socket file's `f_cred`), which scoped
    /// domains compare with theirs.
    landlock: std::sync::OnceLock<Arc<crate::landlock::Domain>>,
    /// `SO_RCVTIMEO`/`SO_SNDTIMEO`.
    pub(crate) timeouts: Timeouts,
    host: Arc<dyn Host>,
    me: Weak<Sock>,
    /// Held by a stream reader from looking at the data until it took it (`u->iolock`), so
    /// what it leaves behind on a fault is still first. Never held while
    /// waiting for data.
    io_lock: futures_util::lock::Mutex<()>,
}

impl Drop for Sock {
    fn drop(&mut self) {
        self.unlist();
    }
}

/// A line of `/proc/net/unix`: what `unix_seq_show` reads of a socket.
pub(crate) struct SeqEntry {
    /// `sk_refcnt`.
    pub(crate) refs: usize,
    /// `sk_state == TCP_LISTEN`.
    pub(crate) listening: bool,
    pub(crate) socket_type: SocketType,
    /// `sk_state == TCP_ESTABLISHED`.
    pub(crate) established: bool,
    /// The inode of the socket's file, 0 while it has none (`sk_socket`).
    pub(crate) ino: u64,
    /// The bound address, family included.
    pub(crate) addr: Option<Vec<u8>>,
}

/// What `unix_diag`'s `sk_diag_fill` reads of a socket.
pub(crate) struct DiagEntry {
    /// The socket's cookie (`sock_gen_cookie`), never reused.
    pub(crate) cookie: u64,
    pub(crate) ino: u64,
    pub(crate) socket_type: SocketType,
    /// `sk_state`: `TCP_LISTEN`, `TCP_ESTABLISHED` or `TCP_CLOSE`.
    pub(crate) state: u8,
    /// The bound address, family included.
    pub(crate) addr: Option<Vec<u8>>,
    pub(crate) vfs: Option<(u64, wasixv2_vfs::Device)>,
    /// The peer's inode, if connected.
    pub(crate) peer: Option<u64>,
    /// A listener's queued connections, by their clients' inodes.
    pub(crate) icons: Vec<u64>,
    /// `unix_diag_rqlen`: a listener's queue and backlog, else `unix_inq_len` and
    /// `unix_outq_len`.
    pub(crate) rqlen: (u32, u32),
    pub(crate) shutdown: u8,
    pub(crate) uid: u32,
    /// `SK_MEMINFO_*`.
    pub(crate) meminfo: [u32; 9],
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
        net: Arc<NetNs>,
        owner: PeerCred,
        host: Arc<dyn Host>,
    ) -> Arc<Self> {
        let ty = match kind {
            Kind::Stream => SocketKind::Stream,
            Kind::Dgram | Kind::SeqPacket => SocketKind::Datagram,
        };
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let sock = Arc::new_cyclic(|me| Self {
            id,
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
                established: false,
                ino: 0,
                vfs: None,
                opts: SockOpts::unix(ty),
                peek_off: -1,
            }),
            wait: WaitQueue::new(),
            fasync: SockAsync::default(),
            net,
            owner,
            landlock: std::sync::OnceLock::new(),
            timeouts: Timeouts::default(),
            host,
            me: me.clone(),
            io_lock: futures_util::lock::Mutex::new(()),
        });
        let mut sockets = sock.names().sockets.lock_or_recover();
        sockets.insert(id, Arc::downgrade(&sock));
        drop(sockets);
        sock
    }

    /// `sk_data_ready` (`sock_def_readable`): data, or a connection to accept, arrived.
    fn data_ready(&self) {
        self.wait.wake(PollMask::READABLE);
        self.fasync.wake(SockWake::WaitData, SiCode::POLL_IN);
    }

    /// `sk_write_space` (`unix_write_space`): what this socket sent was taken.
    fn write_space(&self) {
        self.wait.wake(PollMask::WRITABLE);
        self.fasync.wake(SockWake::Space, SiCode::POLL_OUT);
    }

    /// Takes the socket off its namespace's list (`unix_remove_socket`).
    fn unlist(&self) {
        self.names().sockets.lock_or_recover().remove(&self.id);
    }

    /// The socket has a file, whose inode is `ino` (`sock_graft`).
    pub(crate) fn graft(&self, ino: u64) {
        self.lock().ino = ino;
    }

    /// The socket as `/proc/net/unix` shows it (`unix_seq_show`); `refs` counts the
    /// references to it besides the caller's.
    pub(crate) fn seq_entry(&self, refs: usize) -> SeqEntry {
        let inner = self.lock();
        let established = match self.kind {
            Kind::Dgram => inner.established,
            Kind::Stream | Kind::SeqPacket => inner.state == State::Connected,
        };
        SeqEntry {
            refs,
            listening: inner.state == State::Listening,
            socket_type: self.kind.socket_type(),
            established,
            ino: inner.ino,
            addr: inner.addr.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_or_recover()
    }

    /// The socket as `unix_diag` reports it. Peers are looked at after this socket's lock is
    /// gone, as `unix_peer_get` takes a reference first.
    pub(crate) fn diag_entry(&self) -> DiagEntry {
        const TCP_ESTABLISHED: u8 = 1;
        const TCP_CLOSE: u8 = 7;
        const TCP_LISTEN: u8 = 10;
        let inner = self.lock();
        let listening = inner.state == State::Listening;
        let established = match self.kind {
            Kind::Dgram => inner.established,
            Kind::Stream | Kind::SeqPacket => inner.state == State::Connected,
        };
        let state = match (listening, established) {
            (true, _) => TCP_LISTEN,
            (false, true) => TCP_ESTABLISHED,
            (false, false) => TCP_CLOSE,
        };
        // `unix_inq_len`: every byte for a stream or seqpacket socket, the first datagram's.
        let inq = match self.kind {
            Kind::Dgram => inner.rx.front().map_or(0, |m| m.data.len()),
            Kind::Stream | Kind::SeqPacket => inner.rx_bytes,
        };
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        let embryos: Vec<_> = inner.accept.iter().cloned().collect();
        let entry = DiagEntry {
            cookie: self.id,
            ino: inner.ino,
            socket_type: self.kind.socket_type(),
            state,
            addr: inner.addr.clone(),
            vfs: inner.vfs,
            peer: None,
            icons: Vec::new(),
            rqlen: match listening {
                true => (inner.accept.len() as u32, inner.backlog as u32),
                false => (inq as u32, 0),
            },
            shutdown: inner.shutdown,
            uid: self.owner.ucred.uid,
            meminfo: [
                inner.rx_bytes as u32,
                inner.opts.rcvbuf as u32,
                0,
                inner.opts.sndbuf as u32,
                0,
                0,
                0,
                0,
                0,
            ],
        };
        drop(inner);
        let outq = match (&peer, listening) {
            (Some(peer), false) if self.kind != Kind::Dgram => peer.lock().rx_bytes,
            _ => 0,
        };
        let icons = embryos
            .iter()
            .filter_map(|embryo| embryo.peer())
            .map(|client| client.lock().ino)
            .collect();
        DiagEntry {
            peer: peer.map(|p| p.lock().ino),
            icons,
            rqlen: match listening {
                true => entry.rqlen,
                false => (entry.rqlen.0, outq as u32),
            },
            meminfo: {
                let mut m = entry.meminfo;
                m[2] = outq as u32;
                m
            },
            ..entry
        }
    }

    /// The network namespace the socket was made in (`sock_net`).
    pub(crate) fn net(&self) -> &Arc<NetNs> {
        &self.net
    }

    /// Where the socket's namespace finds bound sockets.
    fn names(&self) -> &Names {
        &self.net.unix_names
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
            match x.kind.connection() {
                true => inner.state = State::Connected,
                false => inner.established = true,
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
            inner.established = false;
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
        self.unlist();
        if let Some(key) = key {
            self.names().remove(&key, self);
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
            if self.kind.connection() {
                peer.fasync.wake(SockWake::WaitData, SiCode::POLL_HUP);
            }
        }
        // Connections nobody accepted go with the listener.
        for embryo in accept {
            embryo.release(true);
        }
        self.wait.wake(PollMask::empty());
        drop(rx);
        // `unix_schedule_gc(NULL)`: this may have left sockets only messages hold.
        self.names().inflight.released();
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
        self.names().insert(key.clone(), &me)?;
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
                let mut next = self.names().next.lock_or_recover();
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
            self.net.clone(),
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
        target.data_ready();
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
        // `unix_dgram_connect` sets no peer credentials. Connecting makes both ends
        // established; disconnecting closes this end, and the old peer if it has no peer.
        if let Some(target) = &target {
            target.lock().established = true;
        }
        let old = {
            let mut inner = self.lock();
            let old = std::mem::replace(&mut inner.peer, target.as_ref().map(Arc::downgrade));
            match &target {
                Some(_) => inner.established = true,
                None if old.is_some() => inner.established = false,
                None => {}
            }
            old.and_then(|old| old.upgrade())
        };
        if let Some(old) = old
            && target
                .as_ref()
                .is_none_or(|target| !Arc::ptr_eq(target, &old))
        {
            let mut old = old.lock();
            if old.peer.is_none() {
                old.established = false;
            }
        }
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
        cred: &Ucred,
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
        // A record goes through the receiver's filter first (`unix_dgram_sendmsg`); one it
        // drops is lost, and the send succeeds.
        let keep = match self.kind {
            Kind::Stream => data.len(),
            _ => match target.opts.filter.trim_message(data) {
                Some(keep) => keep,
                None => return Ok(data.len()),
            },
        };
        if target.released || target.shutdown & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if !self.room_at(sndbuf, &target) {
            self.fasync.set_nospace();
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
            _ => {
                let n = data.len();
                data.truncate(keep);
                (n, std::mem::take(data))
            }
        };
        target.rx_bytes += bytes.len();
        target.rx.push_back(Msg {
            urg: Urg::None,
            data: bytes,
            read: 0,
            from,
            rights: InFlight::new(
                std::mem::take(&mut anc.rights),
                anc.uid,
                &self.names().inflight,
            ),
            cred: anc.cred.as_ref().unwrap_or(cred).clone(),
        });
        drop(target);
        peer.data_ready();
        Ok(n)
    }

    /// `queue_oob`: one attempt to queue `byte` at the peer as its out-of-band byte, which
    /// leaves an earlier one ordinary data. The files not yet sent go with it.
    fn try_send_oob(&self, byte: u8, anc: &mut Ancillary, cred: Ucred) -> Result<usize, Errno> {
        let (peer, sndbuf, from) = {
            let mut inner = self.lock();
            if inner.state != State::Connected {
                return Err(Errno::ENOTCONN);
            }
            if let Some(err) = inner.err.take() {
                return Err(err);
            }
            if inner.shutdown & SEND_SHUTDOWN != 0 {
                return Err(Errno::EPIPE);
            }
            (inner.peer.clone(), inner.opts.sndbuf, inner.addr.clone())
        };
        let peer = peer.and_then(|p| p.upgrade()).ok_or(Errno::EPIPE)?;
        let mut target = peer.lock();
        if target.released || target.shutdown & RCV_SHUTDOWN != 0 {
            return Err(Errno::EPIPE);
        }
        if !self.room_at(sndbuf, &target) {
            return Err(Errno::EAGAIN);
        }
        for msg in target.rx.iter_mut().filter(|m| m.urg == Urg::Mark) {
            msg.urg = Urg::None;
        }
        target.rx_bytes += 1;
        target.rx.push_back(Msg {
            urg: Urg::Mark,
            data: vec![byte],
            read: 0,
            from,
            rights: InFlight::new(
                std::mem::take(&mut anc.rights),
                anc.uid,
                &self.names().inflight,
            ),
            cred: anc.cred.unwrap_or(cred),
        });
        drop(target);
        peer.wait.wake(PollMask::READABLE | PollMask::PRI);
        Ok(1)
    }

    /// `recv(MSG_OOB)` on a stream (`unix_stream_recv_urg`), never waiting: `EINVAL` unless
    /// connected and an out-of-band byte waits.
    pub(crate) fn recv_urg(&self, peek: bool) -> Result<Received, Errno> {
        let mut inner = self.lock();
        if inner.state != State::Connected {
            return Err(Errno::EINVAL);
        }
        let out = take_urg(&mut inner, peek)?;
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        let passcred = inner.passcred();
        drop(inner);
        if !peek && let Some(peer) = peer {
            peer.wait.wake(PollMask::WRITABLE);
        }
        Ok(Received {
            cred: out.cred.filter(|_| passcred),
            ..out
        })
    }

    /// `SIOCATMARK`: whether the next byte read is the out-of-band one's place: the byte
    /// itself, or a spent mark with no other byte waiting or one right after it.
    fn at_mark(&self) -> Result<bool, Errno> {
        if self.kind != Kind::Stream {
            return Err(Errno::EOPNOTSUPP);
        }
        let inner = self.lock();
        let Some(head) = inner.rx.front() else {
            return Ok(false);
        };
        let next_is_mark = inner.rx.get(1).is_some_and(|m| m.urg == Urg::Mark);
        Ok(head.urg == Urg::Mark || (head.urg == Urg::Spent && (!inner.has_oob() || next_is_mark)))
    }

    /// `unix_may_passcred` then `unix_autobind`: a socket that asks for credentials gets a
    /// name before it connects or sends, so its peer can tell who it is.
    pub(crate) fn autobind_for_creds(&self) -> Result<(), Errno> {
        if self.lock().passcred() && !self.is_bound() {
            return self.autobind();
        }
        Ok(())
    }

    /// One attempt to queue a datagram at `target` (`unix_dgram_sendmsg`). The datagram moves
    /// out of `data` when queued.
    fn try_send_dgram(
        &self,
        target: &Arc<Sock>,
        data: &mut Vec<u8>,
        anc: &mut Ancillary,
        cred: &Ucred,
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
        // The receiver's filter comes first; what it drops is lost, and the send succeeds.
        let Some(keep) = other.opts.filter.trim_message(data) else {
            return Ok(data.len());
        };
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
                inner.established = false;
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
            self.fasync.set_nospace();
            return Err(Errno::EAGAIN);
        }
        let len = data.len();
        data.truncate(keep);
        other.rx_bytes += keep;
        other.rx.push_back(Msg {
            urg: Urg::None,
            data: std::mem::take(data),
            read: 0,
            from,
            rights: InFlight::new(
                std::mem::take(&mut anc.rights),
                anc.uid,
                &self.names().inflight,
            ),
            cred: anc.cred.as_ref().unwrap_or(cred).clone(),
        });
        drop(other);
        target.data_ready();
        Ok(len)
    }

    /// One attempt to receive at most `max` bytes (`unix_stream_read_generic`,
    /// `__unix_dgram_recvmsg`).
    fn try_recv(
        &self,
        max: usize,
        flags: MsgFlags,
        writer: Option<&Ucred>,
    ) -> Result<Received, Errno> {
        let take = match flags.contains(MsgFlags::MSG_PEEK) {
            true => Take::Peek,
            false => Take::Consume,
        };
        self.try_take(max, take, writer)
    }

    fn try_take(&self, max: usize, take: Take, writer: Option<&Ucred>) -> Result<Received, Errno> {
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
            Kind::Stream => stream_take(&mut inner, max, take, writer),
            _ => record_take(&mut inner, max, take != Take::Consume),
        };
        // Nothing past the peek offset, or only out-of-band bytes the read skips: the read
        // waits as for an empty queue (`unix_stream_read_generic`'s `again`).
        let Some(out) = out else {
            if let Some(err) = inner.err.take() {
                return Err(err);
            }
            if inner.shutdown & RCV_SHUTDOWN != 0 {
                return Ok(Received::eof());
            }
            return Err(Errno::EAGAIN);
        };
        let peer = inner.peer.as_ref().and_then(Weak::upgrade);
        let passcred = inner.passcred();
        drop(inner);
        if take == Take::Peek {
            self.names().inflight.peeked(&out.rights);
        }
        if take == Take::Consume {
            // Room at this socket: wake senders waiting on it or on their own queue.
            self.wait.wake(PollMask::WRITABLE);
            if let Some(peer) = peer {
                peer.write_space();
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
            if mirrored == RCV_SHUTDOWN | SEND_SHUTDOWN {
                peer.fasync.wake(SockWake::WaitData, SiCode::POLL_HUP);
            } else if mirrored & RCV_SHUTDOWN != 0 {
                peer.fasync.wake(SockWake::WaitData, SiCode::POLL_IN);
            }
        }
        self.wait.wake(PollMask::empty());
    }

    /// `sk_setsockopt` for a Unix socket; the socket layer has taken the timeouts and the
    /// read-only options already. `net_admin` is `CAP_NET_ADMIN`, which the forcing buffer
    /// sizes need.
    pub(crate) fn set_option(&self, name: i32, value: &[u8], net_admin: bool) -> Result<(), Errno> {
        let level = SocketLevel::SOL_SOCKET.0;
        let mut inner = self.lock();
        if forcing_buffer(name) && !net_admin {
            return Err(Errno::EPERM);
        }
        let changed = match SocketOption(name) {
            SocketOption::SO_PEEK_OFF => {
                inner.peek_off = opts::read_int(value)?;
                Changed::Nothing
            }
            _ => {
                let bound = inner.addr.is_some();
                inner.opts.set(level, name, value, bound)?
            }
        };
        drop(inner);
        if changed == Changed::Buffers {
            // More room may be there now.
            self.write_space();
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

/// What a receive does with the data it finds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    /// Takes it, with the files it carries.
    Consume,
    /// `MSG_PEEK`: copies it and the files, and leaves both queued.
    Peek,
    /// Copies the data only, for a reader that takes it once it reached the user.
    Look,
}

/// A stream read of at most `max` bytes from consecutive messages
/// (`unix_stream_read_generic`): it ends after a message that carried files, and with
/// `SO_PASSCRED` it never glues messages of different writers, the first of them being
/// `writer` when the read continues one (`MSG_WAITALL`). A whole unread message moves out
/// without a copy. A peek starts `SO_PEEK_OFF` bytes in. The out-of-band byte ends a read that
/// has data, and at the start of one is skipped, dropped by a read that takes, unless
/// `SO_OOBINLINE` makes it data (`manage_oob`); a spent one ends a read too, unless another
/// waits. `None` when nothing was there to read.
fn stream_take(
    inner: &mut Inner,
    max: usize,
    take: Take,
    writer: Option<Ucred>,
) -> Option<Received> {
    if max == 0 {
        // Whether a read would find anything, for a reader about to wait.
        return stream_take(inner, 1, Take::Look, writer).map(|_| Received::eof());
    }
    let check_creds = inner.passcred();
    let inline = inner.oob_inline();
    let mut skip = match take {
        Take::Peek => inner.peek_off.max(0) as usize,
        _ => 0,
    };
    let mut writer = writer;
    let mut out = Received::eof();
    let mut index = 0;
    let mut any = false;
    while out.data.len() < max && index < inner.rx.len() {
        let copied = !out.data.is_empty();
        match inner.rx[index].urg {
            Urg::Spent => {
                if copied && !inner.has_oob() {
                    break;
                }
                match take {
                    Take::Consume => drop(inner.rx.remove(index)),
                    _ => index += 1,
                }
                continue;
            }
            Urg::Mark if copied => break,
            Urg::Mark if take == Take::Consume => {
                inner.rx[index].urg = Urg::None;
                if !inline {
                    let msg = inner.rx.remove(index).expect("in range");
                    inner.rx_bytes -= msg.data.len() - msg.read;
                    continue;
                }
            }
            Urg::Mark if !inline => {
                index += 1;
                continue;
            }
            Urg::Mark | Urg::None => {}
        }
        let msg = &mut inner.rx[index];
        let avail = msg.data.len() - msg.read;
        if skip >= avail && avail > 0 {
            skip -= avail;
            index += 1;
            continue;
        }
        any = true;
        if check_creds {
            match &writer {
                Some(w) if *w != msg.cred => break,
                Some(_) => {}
                None => writer = Some(msg.cred.clone()),
            }
        }
        if out.cred.is_none() {
            out.cred = Some(msg.cred.clone());
        }
        let had_rights = !msg.rights.is_empty();
        let n = (avail - skip).min(max - out.data.len());
        if take != Take::Consume {
            let at = msg.read + skip;
            out.data.extend(&msg.data[at..at + n]);
            if take == Take::Peek {
                out.rights.extend(msg.rights.files.iter().cloned());
            }
            skip = 0;
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
    match take {
        Take::Peek => inner.move_peek_off(out.data.len() as isize),
        Take::Consume => inner.move_peek_off(-(out.data.len() as isize)),
        Take::Look => {}
    }
    out.full_len = out.data.len();
    any.then_some(out)
}

/// `unix_stream_recv_urg`: the out-of-band byte, with `MSG_OOB`; `EINVAL` without one or
/// with `SO_OOBINLINE`. Taking it leaves its place marked, and drops a spent mark before it.
fn take_urg(inner: &mut Inner, peek: bool) -> Result<Received, Errno> {
    if inner.oob_inline() {
        return Err(Errno::EINVAL);
    }
    let index = inner
        .rx
        .iter()
        .position(|m| m.urg == Urg::Mark)
        .ok_or(Errno::EINVAL)?;
    let msg = &mut inner.rx[index];
    let byte = msg.data[msg.read..].to_vec();
    let cred = msg.cred.clone();
    if !peek {
        msg.urg = Urg::Spent;
        msg.data.clear();
        msg.read = 0;
        inner.rx_bytes -= byte.len();
        if index > 0 && inner.rx[index - 1].urg == Urg::Spent {
            inner.rx.remove(index - 1);
        }
    }
    Ok(Received {
        full_len: byte.len(),
        data: byte,
        from: None,
        flags: MsgFlags::MSG_OOB,
        rights: Vec::new(),
        cred: Some(cred),
    })
}

/// A datagram or seqpacket read: one whole message, truncated to `max`. A peek with
/// `SO_PEEK_OFF` reads from that far into the queue, past whole messages and into one
/// (`__skb_try_recv_from_queue`); `None` when the offset is past them all.
fn record_take(inner: &mut Inner, max: usize, peek: bool) -> Option<Received> {
    if peek {
        let mut off = inner.peek_off.max(0) as usize;
        let mut found = None;
        for (i, msg) in inner.rx.iter().enumerate() {
            let len = msg.data.len();
            if off >= len && (len > 0 || off > 0) {
                off -= len;
                continue;
            }
            found = Some(i);
            break;
        }
        let msg = &inner.rx[found?];
        let full_len = msg.data.len() - off;
        let data = msg.data[off..off + full_len.min(max)].to_vec();
        let mut flags = MsgFlags(0);
        if data.len() < full_len {
            flags |= MsgFlags::MSG_TRUNC;
        }
        let out = Received {
            full_len,
            from: msg.from.clone(),
            flags,
            rights: msg.rights.files.clone(),
            cred: Some(msg.cred.clone()),
            data,
        };
        inner.move_peek_off(out.data.len() as isize);
        return Some(out);
    }
    let mut msg = inner.rx.pop_front()?;
    inner.rx_bytes -= msg.data.len();
    inner.move_peek_off(-(msg.data.len() as isize));
    let rights = msg.rights.take();
    let mut data = std::mem::take(&mut msg.data);
    let full_len = data.len();
    data.truncate(max);
    let mut flags = MsgFlags(0);
    if data.len() < full_len {
        flags |= MsgFlags::MSG_TRUNC;
    }
    Some(Received {
        data,
        full_len,
        from: msg.from.take(),
        flags,
        rights,
        cred: Some(msg.cred.clone()),
    })
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
        if inner.has_oob() {
            mask |= PollMask::PRI;
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

    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.0.blocking(io, RCV, MsgFlags(0));
            if self.0.kind != Kind::Stream {
                let got = recv(&self.0, &mut wait, buf.len(), MsgFlags(0), None).await?;
                return delivered(got.len(), to.copy_out(&got.data).await);
            }
            let got = recv_stream_to(&self.0, &mut wait, buf.len(), None, to).await?;
            Ok(got.len())
        })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let owner = self.0.owner.ucred.clone();
            let mut wait = self.0.blocking(io, SND, MsgFlags(0));
            let anc = Ancillary::default();
            send(&self.0, &mut wait, data.to_vec(), None, anc, owner).await
        })
    }

    fn buffered(&self) -> usize {
        self.0.input_len()
    }

    /// `sock_fasync`.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        Some(self.0.fasync.fasync(fasync))
    }

    /// `sock_splice_read`.
    fn splice_read(&self) -> bool {
        true
    }

    /// `splice_to_socket`.
    fn splice_write(&self) -> bool {
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
                IoctlRequest::SIOCATMARK => usize::from(self.0.at_mark()?),
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
    writer: Option<&Ucred>,
) -> SysResult<Received> {
    wait.wait(WaitOn::Queue(&sock.wait), PollMask::READABLE, || {
        sock.try_recv(max, flags, writer)
    })
    .await
}

/// A stream receive of at most `max` bytes into `to` that takes them only once all arrived
/// there, as `unix_stream_read_generic` consumes an skb only once `unix_stream_read_actor`
/// copied it: a fault is `EFAULT` with everything still queued ([`taken`]).
/// Waits without the I/O lock (`unix_stream_data_wait`), then looks again under it. `writer`
/// is as for [`recv`].
pub(crate) async fn recv_stream_to(
    sock: &Sock,
    wait: &mut Blocking<'_>,
    max: usize,
    writer: Option<&Ucred>,
    to: &mut dyn ReadSink,
) -> SysResult<Received> {
    loop {
        wait.wait(WaitOn::Queue(&sock.wait), PollMask::READABLE, || {
            sock.try_take(0, Take::Look, writer)
        })
        .await?;
        let _io = sock.io_lock.lock().await;
        let look = match sock.try_take(max, Take::Look, writer) {
            Err(Errno::EAGAIN) => continue,
            Err(err) => return Err(err.into()),
            Ok(look) if look.data.is_empty() => return Ok(look),
            Ok(look) => look,
        };
        let n = taken(look.len(), to.copy_out(&look.data).await)?;
        let got = sock.try_take(n, Take::Consume, writer)?;
        debug_assert_eq!(got.len(), n, "the I/O lock keeps the data in place");
        return Ok(got);
    }
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
                sock.try_send_dgram(&target, &mut data, &mut anc, &cred)
            })
            .await;
    }
    let total = data.len();
    let mut sent = 0;
    loop {
        let n = match wait
            .wait(WaitOn::Queue(&sock.wait), PollMask::WRITABLE, || {
                sock.try_send_connected(&mut data, &mut anc, &cred)
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

/// Sends `byte` as the stream's out-of-band byte, waiting for room unless non-blocking.
pub(crate) async fn send_oob(
    sock: &Sock,
    wait: &mut Blocking<'_>,
    byte: u8,
    mut anc: Ancillary,
    cred: Ucred,
) -> SysResult<usize> {
    wait.wait(WaitOn::Queue(&sock.wait), PollMask::WRITABLE, || {
        sock.try_send_oob(byte, &mut anc, cred)
    })
    .await
}

impl Kernel {
    /// The credentials `SCM_CREDENTIALS` and `SO_PEERCRED` report for the caller.
    pub(crate) fn ucred(&self, caller: &Caller<'_>) -> Ucred {
        let process = self.process(caller.task());
        let (uid, gid) = process.cred().ucred_ids();
        Ucred {
            pid: Some(self.pids.get(process.pid.0)),
            uid: uid.0,
            gid: gid.0,
        }
    }

    /// `cred_to_ucred` and `scm_recv`'s `pid_vnr`: `cred` as the caller reads it, with the pid
    /// it sees for the process (0 outside its pid namespace; still its number once it was
    /// reaped) and the ids its user namespace sees for them; the invalid ids of no peer stay.
    pub(crate) fn ucred_bytes(&self, caller: &Caller<'_>, cred: &Ucred) -> Vec<u8> {
        let ns = self.active_pid_ns(caller.task());
        let pid = cred.pid.as_ref().map_or(0, |pid| pid.nr(&ns));
        let ns = self.cred(caller).user_ns;
        let (uid, gid) = match cred.uid == Ucred::NONE.uid {
            true => (cred.uid, cred.gid),
            false => (ns.munged_uid(Kuid(cred.uid)), ns.munged_gid(Kgid(cred.gid))),
        };
        RawUcred { pid, uid, gid }.to_bytes()
    }

    /// The caller as a socket's peer: what `SO_PEERCRED` and `SO_PEERGROUPS` report
    /// (`prepare_peercred`).
    pub(crate) fn peer_cred(&self, caller: &Caller<'_>) -> PeerCred {
        PeerCred {
            ucred: self.ucred(caller),
            groups: self.cred(caller).groups.as_slice().into(),
        }
    }

    /// `scm_check_creds`: a process may claim its own pid and any of its real, effective or
    /// saved ids, and others with `CAP_SYS_ADMIN` over its pid namespace's owner, `CAP_SETUID`
    /// and `CAP_SETGID` over its own user namespace. `claim` has
    /// the guest's ids, which its user namespace must map (`EINVAL`); the kernel ids come back.
    fn may_claim(&self, caller: &Caller<'_>, claim: &RawUcred) -> Result<(Kuid, Kgid), Errno> {
        let process = self.process(caller.task());
        let cred = process.cred();
        let uid = cred.user_ns.make_kuid(claim.uid).ok_or(Errno::EINVAL)?;
        let gid = cred.user_ns.make_kgid(claim.gid).ok_or(Errno::EINVAL)?;
        let allowed = (claim.pid == self.pids.own_nr(process.pid.0)
            || cred.ns_capable(process.pid_ns.user_ns(), Cap::SysAdmin))
            && (cred.user.holds(uid) || cred.capable(Cap::Setuid))
            && (cred.group.holds(gid) || cred.capable(Cap::Setgid));
        match allowed {
            true => Ok((uid, gid)),
            false => Err(Errno::EPERM),
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
        let sock = Sock::new(
            kind,
            self.ns(caller).net.clone(),
            self.peer_cred(caller),
            self.host.clone(),
        );
        if let Some(domain) = self.landlock(caller) {
            let _ = sock.landlock.set(domain);
        }
        Ok(sock)
    }

    /// Finds the socket a name refers to for `sock` (`unix_find_other`): an abstract name in
    /// the network namespace `sock` was made in, a path wherever the caller's walk leads.
    async fn unix_find(
        &self,
        caller: &Caller<'_>,
        sock: &Sock,
        name: &Name,
    ) -> SysResult<Arc<Sock>> {
        let landlock = self.landlock(caller);
        match name {
            Name::Abstract(name) => {
                let other = sock
                    .names()
                    .find(&Key::Abstract(name.clone()))
                    .ok_or(Errno::ECONNREFUSED)?;
                // `hook_unix_stream_connect`, `hook_unix_may_send`: only a pathname socket
                // reaches out of a domain that scopes abstract sockets.
                if let Some(landlock) = &landlock
                    && landlock.scoped(
                        other.landlock.get(),
                        crate::landlock::SCOPE_ABSTRACT_UNIX_SOCKET,
                    )
                {
                    return Err(Errno::EPERM.into());
                }
                Ok(other)
            }
            Name::Path(path) => {
                let env = self.env(caller);
                let start = self.start(caller, &env, wasixv2_syscalls::Fd(-100), path)?;
                let found = self
                    .vfs
                    .lookup(&env.cx(), &start, path, LookupFlags::follow())
                    .await?;
                self.vfs.permission(&env.cx(), &found, MAY_WRITE).await?;
                if found.dentry.file_type() != FileType::Socket {
                    return Err(Errno::ECONNREFUSED.into());
                }
                let key = Key::Inode(found.dentry.sb().id, found.dentry.node().ino());
                let other = sock.names().find(&key).ok_or(Errno::ECONNREFUSED)?;
                // `security_unix_find`.
                if let Some(landlock) = &landlock {
                    landlock.check_unix_find(&found, other.landlock.get())?;
                }
                Ok(other)
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
        sock.lock().vfs = Some((found.dentry.node().ino(), found.dentry.sb().dev));
        Ok(0)
    }

    /// `unix_stream_connect` and `unix_dgram_connect`.
    pub(crate) async fn unix_connect(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &Sock,
        raw: &[u8],
        wait: MsgFlags,
    ) -> SysResult<i32> {
        if sock.kind == Kind::Dgram && crate::socket::family(raw)? == AddressFamily::AF_UNSPEC {
            sock.connect_dgram(None)?;
            return Ok(0);
        }
        let name = Name::decode(raw)?.ok_or(Errno::EINVAL)?;
        sock.autobind_for_creds()?;
        let target = self.unix_find(caller, sock, &name).await?;
        if sock.kind == Kind::Dgram {
            sock.connect_dgram(Some(target))?;
            return Ok(0);
        }
        let cred = self.peer_cred(caller);
        let interrupt = self.interrupt(caller);
        let mut wait = sock.blocking(Self::io(caller, &*interrupt, file), SND, wait);
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
        Ok(Some(self.unix_find(caller, sock, &name).await?))
    }

    /// `__scm_send` for a Unix socket: `SCM_RIGHTS` and `SCM_CREDENTIALS`.
    pub(crate) fn unix_ancillary(
        &self,
        caller: &Caller<'_>,
        controls: &[wasixv2_vnet::Control],
    ) -> SysResult<Ancillary> {
        let mut anc = Ancillary {
            uid: self.cred(caller).user.real.0,
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
                    let cred = RawUcred::from_bytes(&control.data)
                        .filter(|_| control.data.len() == 12)
                        .ok_or(Errno::EINVAL)?;
                    let (uid, gid) = self.may_claim(caller, &cred)?;
                    // `find_get_pid`: the claimed process, in the caller's pid namespace.
                    let pid = self.find_vpid(caller, cred.pid).ok_or(Errno::ESRCH)?;
                    anc.cred = Some(Ucred {
                        pid: Some(self.pids.get(pid)),
                        uid: uid.0,
                        gid: gid.0,
                    });
                }
                _ => return Err(Errno::EINVAL.into()),
            }
        }
        // `unix_schedule_gc(user)`: a user with many files in flight sending Unix sockets
        // collects first.
        let ns = self.ns(caller);
        let inflight = &ns.net.unix_names.inflight;
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
    /// `RLIMIT_NOFILE`, and is not exempt through `CAP_SYS_RESOURCE` or `CAP_SYS_ADMIN` over
    /// the initial user namespace.
    fn too_many_unix_fds(&self, caller: &Caller<'_>, uid: u32) -> bool {
        let limit = self.files(caller.task()).lock_or_recover().limit();
        if self.ns(caller).net.unix_names.inflight.user(uid) <= limit {
            return false;
        }
        let cred = self.process(caller.task()).cred();
        !cred.capable_init(Cap::SysResource) && !cred.capable_init(Cap::SysAdmin)
    }

    /// `sk_getsockopt` for a Unix socket, the timeouts aside: what describes the socket, its
    /// peer's credentials, and the options `setsockopt` keeps. Values are written whole; the
    /// caller truncates them to the guest's length.
    pub(crate) fn unix_getsockopt(
        &self,
        caller: &Caller<'_>,
        sock: &Sock,
        name: i32,
    ) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(sock.kind.socket_type().0),
            SocketOption::SO_PROTOCOL => int(0),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_UNIX.0),
            SocketOption::SO_ERROR => int(sock.take_error().map_or(0, |e| e.0)),
            SocketOption::SO_ACCEPTCONN => int(sock.is_listening() as i32),
            SocketOption::SO_PEEK_OFF => int(sock.lock().peek_off),
            // `cred_to_ucred`: without a peer, pid 0 and the invalid ids.
            SocketOption::SO_PEERCRED => {
                let peer = sock.peer_cred().map_or(Ucred::NONE, |peer| peer.ucred);
                Ok(self.ucred_bytes(caller, &peer))
            }
            // `SO_PEERGROUPS` is answered by the socket layer, which may need `ERANGE`.
            _ => {
                // `SO_GET_FILTER` writes the whole program.
                let size = match name == wasixv2_vnet::bpf::SO_GET_FILTER {
                    true => wasixv2_vnet::bpf::MAX_PROGRAM_LEN,
                    false => 64,
                };
                let mut out = vec![0u8; size];
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
/// `SO_SNDBUFFORCE` and `SO_RCVBUFFORCE`, which `sk_setsockopt` allows only with
/// `CAP_NET_ADMIN` (`sockopt_capable`), for every family.
pub(crate) fn forcing_buffer(name: i32) -> bool {
    matches!(
        SocketOption(name),
        SocketOption::SO_SNDBUFFORCE | SocketOption::SO_RCVBUFFORCE
    )
}

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

    /// A network namespace with no IP network and the Unix names `names`.
    fn net(names: Arc<Names>) -> Arc<NetNs> {
        crate::ns::Ns::initial(
            crate::ns::NsKind::Net,
            None,
            crate::ns::Net {
                network: None,
                unix_names: names,
                devconf: Default::default(),
                sysctl: Default::default(),
            },
        )
    }

    fn pair(kind: Kind) -> (Arc<Sock>, Arc<Sock>) {
        let net = net(Arc::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let a = Sock::new(kind, net.clone(), PeerCred::default(), host.clone());
        let b = Sock::new(kind, net, PeerCred::default(), host);
        Sock::pair(&a, &b);
        (a, b)
    }

    fn cred(pids: &Arc<crate::pid::Pids>, pid: u32) -> Ucred {
        Ucred {
            pid: Some(pids.get(pid)),
            uid: 1,
            gid: 1,
        }
    }

    #[test]
    fn stream_reads_never_glue_writers_with_so_passcred() {
        let (a, b) = pair(Kind::Stream);
        let pids = Arc::default();
        let cred = |pid| cred(&pids, pid);
        let send = |data: &[u8], pid| {
            let mut data = data.to_vec();
            let mut anc = Ancillary::default();
            a.try_send_connected(&mut data, &mut anc, &cred(pid))
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
            (first.data.as_slice(), &first.cred),
            (&b"ab"[..], &Some(cred(10)))
        );
        // `MSG_WAITALL` going on from the first writer stops at the second's data.
        let more = b.try_recv(16, MsgFlags(0), first.cred.as_ref()).unwrap();
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
            a.try_send_connected(&mut Vec::new(), &mut anc, &Ucred::default()),
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
            a.try_send_connected(&mut data, &mut anc, &Ucred::default()),
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
        let limit = wasixv2_vnet::opts::UDP_BUF_DEFAULT - RECORD_OVERHEAD;
        let mut data = vec![0u8; limit + 1];
        let sent = a.try_send_dgram(&b, &mut data, &mut anc, &Ucred::default());
        assert_eq!(sent, Err(Errno::EMSGSIZE));
        let mut data = vec![0u8; limit];
        assert_eq!(
            a.try_send_dgram(&b, &mut data, &mut anc, &Ucred::default()),
            Ok(limit)
        );
    }

    #[test]
    fn closing_a_listener_resets_queued_clients() {
        let net = net(Arc::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let listener = Sock::new(Kind::Stream, net.clone(), PeerCred::default(), host.clone());
        listener
            .bind(Key::Abstract(b"l".to_vec()), b"\x01\x00\x00l".to_vec())
            .unwrap();
        listener.listen(4, PeerCred::default()).unwrap();
        let client = Sock::new(Kind::Stream, net, PeerCred::default(), host);
        client.try_connect(&listener, &PeerCred::default()).unwrap();
        listener.release(false);
        assert_eq!(
            client.try_recv(4, MsgFlags(0), None).err(),
            Some(Errno::ECONNRESET)
        );
        assert_eq!(client.try_recv(4, MsgFlags(0), None).unwrap().len(), 0);
    }

    #[test]
    fn a_new_network_namespace_has_its_own_abstract_names_only() {
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let names = Arc::new(Names::default());
        let new = Arc::new(names.for_new_net());
        let sock = |names: &Arc<Names>| {
            Sock::new(
                Kind::Dgram,
                net(names.clone()),
                PeerCred::default(),
                host.clone(),
            )
        };
        let (old_abstract, old_path) = (sock(&names), sock(&names));
        let abstract_name = Key::Abstract(b"a".to_vec());
        let path = Key::Inode(1, 2);
        old_abstract
            .bind(abstract_name.clone(), b"\x01\x00\x00a".to_vec())
            .unwrap();
        old_path.bind(path.clone(), b"\x01\x00/s".to_vec()).unwrap();

        assert!(new.find(&abstract_name).is_none());
        assert!(Arc::ptr_eq(&new.find(&path).unwrap(), &old_path));
        assert!(Arc::ptr_eq(&new.inflight, &names.inflight));
        let mine = sock(&new);
        assert_eq!(
            mine.bind(abstract_name.clone(), b"\x01\x00\x00a".to_vec()),
            Ok(())
        );
        let taken = sock(&new).bind(path, b"\x01\x00/s".to_vec());
        assert_eq!(taken, Err(Errno::EADDRINUSE), "one path table");
        assert!(Arc::ptr_eq(
            &names.find(&abstract_name).unwrap(),
            &old_abstract
        ));
        assert!(Arc::ptr_eq(&new.find(&abstract_name).unwrap(), &mine));
    }

    #[test]
    fn connecting_twice_queues_one_connection() {
        let net = net(Arc::default());
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let listener = Sock::new(Kind::Stream, net.clone(), PeerCred::default(), host.clone());
        listener
            .bind(Key::Abstract(b"m".to_vec()), b"\x01\x00\x00m".to_vec())
            .unwrap();
        listener.listen(4, PeerCred::default()).unwrap();
        let client = Sock::new(Kind::Stream, net, PeerCred::default(), host);
        assert_eq!(client.try_connect(&listener, &PeerCred::default()), Ok(()));
        assert_eq!(
            client.try_connect(&listener, &PeerCred::default()),
            Err(Errno::EISCONN)
        );
        assert!(listener.try_accept().is_ok());
        assert_eq!(listener.try_accept().err(), Some(Errno::EAGAIN));
    }

    fn send(from: &Sock, data: &[u8]) {
        let mut data = data.to_vec();
        let mut anc = Ancillary::default();
        from.try_send_connected(&mut data, &mut anc, cred(1))
            .unwrap();
    }

    fn send_oob(from: &Sock, byte: u8) {
        let mut anc = Ancillary::default();
        assert_eq!(from.try_send_oob(byte, &mut anc, cred(1)), Ok(1));
    }

    fn set(sock: &Sock, name: SocketOption, value: i32) {
        sock.set_option(name.0, &value.to_le_bytes(), false)
            .unwrap();
    }

    /// `send(MSG_OOB)` of "abc" on a stream: "ab" is data, "c" the out-of-band byte, which
    /// reads stop before, `recv(MSG_OOB)` takes, `SIOCATMARK` and `POLLPRI` report, and a
    /// read at the mark skips.
    #[test]
    fn stream_out_of_band_bytes_mark_the_stream() {
        let (a, b) = pair(Kind::Stream);
        send(&a, b"ab");
        send_oob(&a, b'c');
        send(&a, b"de");
        let file = UnixFile(b.clone());
        assert!(file.poll(None).contains(PollMask::PRI));
        assert_eq!(b.at_mark(), Ok(false));
        let got = b.try_recv(16, MsgFlags(0), None).unwrap();
        assert_eq!(got.data, b"ab", "a read stops at the mark");
        assert_eq!(b.at_mark(), Ok(true));
        let oob = b.recv_urg(true).unwrap();
        assert_eq!(
            (oob.data.as_slice(), oob.flags),
            (&b"c"[..], MsgFlags::MSG_OOB)
        );
        let oob = b.recv_urg(false).unwrap();
        assert_eq!(oob.data, b"c");
        assert!(!file.poll(None).contains(PollMask::PRI));
        assert_eq!(b.recv_urg(false).err(), Some(Errno::EINVAL));
        assert_eq!(b.at_mark(), Ok(true), "the spent mark is still the place");
        assert_eq!(b.try_recv(16, MsgFlags(0), None).unwrap().data, b"de");
        assert_eq!(b.at_mark(), Ok(false));

        // Unread, the byte is dropped by a read that reaches it, unless SO_OOBINLINE.
        send_oob(&a, b'x');
        send(&a, b"yz");
        assert_eq!(
            b.try_recv(16, MsgFlags::MSG_PEEK, None).unwrap().data,
            b"yz"
        );
        assert_eq!(b.try_recv(16, MsgFlags(0), None).unwrap().data, b"yz");
        assert_eq!(b.try_recv(16, MsgFlags(0), None).err(), Some(Errno::EAGAIN));
        set(&b, SocketOption::SO_OOBINLINE, 1);
        send_oob(&a, b'q');
        assert_eq!(b.recv_urg(false).err(), Some(Errno::EINVAL));
        assert_eq!(b.try_recv(16, MsgFlags(0), None).unwrap().data, b"q");
        // A newer byte leaves an older one ordinary data.
        set(&b, SocketOption::SO_OOBINLINE, 0);
        send_oob(&a, b'1');
        send_oob(&a, b'2');
        assert_eq!(b.try_recv(16, MsgFlags(0), None).unwrap().data, b"1");
        assert_eq!(b.recv_urg(false).unwrap().data, b"2");
        drop(file);
        let (d, _) = pair(Kind::Dgram);
        assert_eq!(d.at_mark(), Err(Errno::EOPNOTSUPP));
    }

    /// `SO_PEEK_OFF`: peeking reads on from where the last peek stopped, reading takes the
    /// offset back; a datagram peek goes past whole datagrams and into one.
    #[test]
    fn peek_offsets_move_with_peeks_and_reads() {
        let (a, b) = pair(Kind::Stream);
        assert_eq!(b.lock().peek_off, -1);
        set(&b, SocketOption::SO_PEEK_OFF, 0);
        send(&a, b"abc");
        send(&a, b"def");
        let peek = |n| b.try_recv(n, MsgFlags::MSG_PEEK, None).map(|r| r.data);
        assert_eq!(peek(2).unwrap(), b"ab");
        assert_eq!(peek(2).unwrap(), b"cd");
        assert_eq!(b.lock().peek_off, 4);
        assert_eq!(b.try_recv(3, MsgFlags(0), None).unwrap().data, b"abc");
        assert_eq!(b.lock().peek_off, 1);
        assert_eq!(peek(8).unwrap(), b"ef");
        assert_eq!(
            peek(8).err(),
            Some(Errno::EAGAIN),
            "nothing past the offset"
        );

        let (x, y) = pair(Kind::Dgram);
        set(&y, SocketOption::SO_PEEK_OFF, 0);
        for d in [&b"one"[..], b"two"] {
            let mut data = d.to_vec();
            x.try_send_dgram(&y, &mut data, &mut Ancillary::default(), cred(1))
                .unwrap();
        }
        let dpeek = |n| y.try_recv(n, MsgFlags::MSG_PEEK, None).unwrap().data;
        assert_eq!(dpeek(2), b"on");
        assert_eq!(dpeek(8), b"e");
        assert_eq!(dpeek(8), b"two");
        assert_eq!(y.try_recv(8, MsgFlags(0), None).unwrap().data, b"one");
        assert_eq!(y.lock().peek_off, 3);
    }

    /// With `SO_PASSCRED`, an unbound socket gets an abstract name before it connects or
    /// sends (`unix_may_passcred`).
    #[test]
    fn so_passcred_autobinds() {
        let (a, _) = pair(Kind::Dgram);
        a.autobind_for_creds().unwrap();
        assert!(!a.is_bound());
        set(&a, SocketOption::SO_PASSCRED, 1);
        a.autobind_for_creds().unwrap();
        let addr = a.local_addr();
        assert_eq!(addr.len(), SUN_PATH + 6);
        assert_eq!(addr[SUN_PATH], 0, "an abstract name");
    }
}
