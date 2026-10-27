//! The interface between the kernel's socket layer, a network namespace and its backend.
//!
//! - The kernel decodes and encodes guest structs, owns descriptors, blocking, timeouts
//!   (`SO_RCVTIMEO`, `SO_SNDTIMEO`), signals and Unix sockets. It talks to a namespace through
//!   `Network` and to each socket through `Socket`.
//! - A `Network` is one network namespace: its port tables, interfaces and in-namespace
//!   connections. It hands traffic for other destinations to a `Backend`.
//! - A `Backend` carries traffic leaving the namespace (host sockets, a virtual switch) and
//!   delivers published inbound connections.
//!
//! Every operation here is synchronous and non-blocking, like Linux's protocol operations
//! called with `O_NONBLOCK`: it either completes or fails with `EAGAIN` (or `EINPROGRESS` for
//! `connect`). Readiness is reported through `Pollable`, and the objects wake their
//! `WaitQueue`s from whatever thread observes the change (a host reactor thread, another guest
//! task). The kernel builds blocking calls from these by waiting for the right poll events and
//! retrying, as Linux's `sk_wait_event` does.
//!
//! Errors are guest-visible `Errno` values with Linux's meaning and precedence.

use std::{fmt, net::SocketAddr, sync::Arc};

use wasixv2_abi::{Errno, MsgFlags};
use wasixv2_wait::{PollTable, Pollable, WaitQueue};

/// The address family of an IP socket. Unix sockets never reach this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Domain {
    /// `AF_INET`
    Inet,
    /// `AF_INET6`
    Inet6,
}

/// The socket type, without the `SOCK_NONBLOCK` and `SOCK_CLOEXEC` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SocketKind {
    /// `SOCK_STREAM`
    Stream,
    /// `SOCK_DGRAM`
    Datagram,
}

/// `shutdown`'s `how`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shutdown {
    Read,
    Write,
    Both,
}

/// One control message (cmsg) of `sendmsg` or `recvmsg`, with its payload in guest layout. The
/// kernel converts between this and the guest's `cmsghdr` array, including alignment and
/// `MSG_CTRUNC`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// `cmsg_level`, such as `SOL_SOCKET` or `SOL_IP`.
    pub level: i32,
    /// `cmsg_type`, such as `IP_PKTINFO` or `SO_TIMESTAMP_NEW`.
    pub kind: i32,
    pub data: Vec<u8>,
}

/// What `Socket::send` sends.
#[derive(Clone, Copy, Debug)]
pub struct SendMsg<'a> {
    /// The bytes, already gathered from the guest's iovecs.
    pub data: &'a [u8],
    /// `sendto`'s address; `None` for a connected socket.
    pub to: Option<SocketAddr>,
    /// The guest's flags. The kernel has handled `MSG_DONTWAIT` and `MSG_NOSIGNAL`; the socket
    /// honours the protocol's own (`MSG_MORE`, `MSG_OOB`, `MSG_EOR`, `MSG_CONFIRM`, ...) or
    /// fails as Linux does for flags the protocol rejects.
    pub flags: MsgFlags,
    /// Control messages from `sendmsg`, such as `IP_PKTINFO` or `IP_TTL`.
    pub control: &'a [Control],
}

/// What `Socket::recv` received.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecvMsg {
    /// Bytes written to the buffer.
    pub len: usize,
    /// The datagram's full length, which `MSG_TRUNC` makes `recv` return; `len` for streams.
    pub full_len: usize,
    /// The sender, for `recvfrom`'s address. `None` for connected streams, where Linux leaves
    /// the address untouched and reports length 0.
    pub from: Option<SocketAddr>,
    /// Output `msg_flags`: `MSG_TRUNC` for a truncated datagram, `MSG_EOR`, `MSG_OOB`.
    pub flags: MsgFlags,
    /// Control messages to deliver, in order (`IP_PKTINFO`, `SO_TIMESTAMP`, ...).
    pub control: Vec<Control>,
}

/// A connection taken off a listener by `Socket::accept`.
pub struct Accepted {
    pub socket: Arc<dyn Socket>,
    /// The peer, as `accept`'s address argument reports it.
    pub peer: SocketAddr,
}

impl fmt::Debug for Accepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accepted")
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

/// One port-table entry, for `/proc/net/{tcp,udp}` and `ss`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortEntry {
    pub kind: SocketKind,
    pub local: SocketAddr,
    pub peer: Option<SocketAddr>,
    pub listening: bool,
}

/// A network interface, for netlink, `SIOCGIF*` and `getifaddrs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    /// The interface index, from 1.
    pub index: u32,
    pub name: String,
    /// `IFF_*` flags (`IFF_UP`, `IFF_LOOPBACK`, `IFF_RUNNING`, ...).
    pub flags: u32,
    pub mtu: u32,
    /// Addresses with their prefix lengths.
    pub addresses: Vec<(std::net::IpAddr, u8)>,
}

/// A network namespace: what the kernel's `socket` and the network pseudo files use.
pub trait Network: Send + Sync {
    /// Linux's `inet_create`/`inet6_create`: a new, unbound socket. `protocol` is the guest's
    /// third `socket` argument: 0 picks the default; an unsupported value is
    /// `EPROTONOSUPPORT`, an unsupported kind/protocol pair `ESOCKTNOSUPPORT` or
    /// `EPROTONOSUPPORT` as Linux decides. `SOCK_RAW` never gets here. `uid` owns the socket
    /// (`sk_uid`, the creator's filesystem uid, which accepted sockets inherit): sockets only
    /// share a port through `SO_REUSEPORT` with sockets of the same owner.
    fn socket(
        &self,
        domain: Domain,
        kind: SocketKind,
        protocol: i32,
        uid: u32,
    ) -> Result<Arc<dyn Socket>, Errno>;

    /// Every bound socket in the namespace.
    fn ports(&self) -> Vec<PortEntry> {
        Vec::new()
    }

    /// The namespace's interfaces; a fresh namespace has only loopback.
    fn interfaces(&self) -> Vec<Interface> {
        Vec::new()
    }
}

/// An IP socket. The kernel wraps it as an open-file object; `poll` must report exactly what
/// Linux's `tcp_poll` and `udp_poll` would (a fresh stream socket is `OUT | WRNORM | HUP`, a
/// listener is `IN | RDNORM` while connections are queued, `RDHUP` after the peer shut down,
/// `ERR` while an error is pending), and every state change must wake the queues `poll`
/// registered with the matching events.
pub trait Socket: Pollable {
    fn domain(&self) -> Domain;
    fn kind(&self) -> SocketKind;
    /// The protocol `SO_PROTOCOL` reports (`IPPROTO_TCP`, `IPPROTO_UDP`, ...), never 0.
    fn protocol(&self) -> i32;

    /// `inet_bind`. `addr`'s family matches `domain` (the kernel checks the guest address);
    /// port 0 picks an ephemeral port. `privileged` says whether the caller may bind ports
    /// below 1024 (`CAP_NET_BIND_SERVICE`); if not, those are `EACCES`. A second bind is
    /// `EINVAL`; a taken address `EADDRINUSE`; an address not on an interface
    /// `EADDRNOTAVAIL`.
    fn bind(&self, addr: SocketAddr, privileged: bool) -> Result<(), Errno>;

    /// `inet_listen`, with `backlog` already clamped to `somaxconn` by the kernel. Binds to an
    /// ephemeral port if unbound. `EOPNOTSUPP` for datagram sockets, `EINVAL` when connected.
    fn listen(&self, backlog: u32) -> Result<(), Errno>;

    /// `inet_accept` without blocking: the next queued connection, `EAGAIN` if none, `EINVAL`
    /// unless listening.
    fn accept(&self) -> Result<Accepted, Errno>;

    /// `inet_stream_connect`/`inet_dgram_connect` without blocking.
    ///
    /// Streams: `Ok` when connected at once, `EINPROGRESS` when the connection is under way
    /// (the socket then becomes writable, or reports `ERR` with `take_error` set, when it
    /// finishes), `EALREADY` while one is in progress, `EISCONN` once connected,
    /// `ECONNREFUSED`, `ENETUNREACH`, ... on failure. Datagrams: set the default peer.
    /// Either binds to an ephemeral port first if unbound.
    fn connect(&self, addr: SocketAddr) -> Result<(), Errno>;

    /// `connect` with `AF_UNSPEC`: dissolve a datagram association, or disconnect a stream.
    fn disconnect(&self) -> Result<(), Errno>;

    /// `inet_shutdown`. `ENOTCONN` on an unconnected stream socket.
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno>;

    /// Sends without blocking. Returns the bytes taken: all of a datagram, or a prefix of a
    /// stream's data. `EAGAIN` when nothing fits, `EPIPE` after `shutdown(Write)` or a reset
    /// (the kernel raises `SIGPIPE`), `EDESTADDRREQ` unconnected without `to`, `EMSGSIZE`
    /// for an oversized datagram. The kernel never passes `to` to a stream socket: TCP
    /// ignores the address.
    fn send(&self, msg: SendMsg<'_>) -> Result<usize, Errno>;

    /// Receives into `buf` without blocking. `flags` may hold `MSG_PEEK`, `MSG_TRUNC`,
    /// `MSG_OOB`, `MSG_ERRQUEUE`; the kernel handles `MSG_WAITALL` and `MSG_DONTWAIT`. Returns
    /// `len == 0` at end of stream; `EAGAIN` when nothing is queued; `ENOTCONN` for an
    /// unconnected stream.
    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> Result<RecvMsg, Errno>;

    /// `getsockname`: the bound address, or the unspecified address and port 0 if unbound.
    fn local_addr(&self) -> Result<SocketAddr, Errno>;

    /// `getpeername`: `ENOTCONN` unless connected.
    fn peer_addr(&self) -> Result<SocketAddr, Errno>;

    /// `sock_error`: takes the pending asynchronous error (`SO_ERROR`), clearing it.
    fn take_error(&self) -> Option<Errno>;

    /// `SIOCINQ`/`FIONREAD`: bytes queued for a stream, the next datagram's size for a
    /// datagram socket; `EINVAL` for a listener.
    fn input_len(&self) -> Result<usize, Errno>;

    /// `SIOCOUTQ`: bytes sent but not yet taken by the peer.
    fn output_len(&self) -> Result<usize, Errno> {
        Ok(0)
    }

    /// `setsockopt` for every option the kernel does not handle itself. `value` is the guest's
    /// bytes, in guest layout, of the length the guest gave. Unknown options are
    /// `ENOPROTOOPT`, short values `EINVAL`, as Linux.
    ///
    /// The kernel handles `SO_TYPE`, `SO_PROTOCOL`, `SO_DOMAIN`, `SO_ERROR`, `SO_RCVTIMEO*`,
    /// `SO_SNDTIMEO*` and every `SOL_SOCKET` option that only describes the descriptor.
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno>;

    /// `getsockopt`: writes the value in guest layout to the start of `out` (whose length is
    /// the guest's `optlen`, at most what the kernel copies back) and returns its length;
    /// Linux truncates most values to `optlen` rather than failing.
    fn get_option(&self, level: i32, name: i32, out: &mut [u8]) -> Result<usize, Errno>;
}

/// Where traffic for destinations outside the namespace goes: host sockets, a virtual switch.
/// Implementations do their I/O off the guest's thread (a reactor).
///
/// A guest socket has one wait queue for its whole life (Linux's `sk_sleep`). A backend object
/// made for it later (on `connect`, on the first datagram to an outside address) wakes that
/// queue, never one of its own, so a waiter that registered on the socket before the backend
/// object existed still hears about it.
pub trait Backend: Send + Sync {
    /// Starts an outbound stream connection to `peer`. The stream reports `OUT` once
    /// connected, or `ERR` with `take_error` set if it failed. Immediate failures
    /// (`ENETUNREACH`) are errors here. The stream wakes `wq`, the guest socket's queue, on
    /// every change of its readiness.
    fn connect(&self, peer: SocketAddr, wq: WaitQueue) -> Result<Arc<dyn BackendStream>, Errno>;

    /// A datagram endpoint for traffic to outside addresses, of the given family. It wakes
    /// `wq`, the guest socket's queue, when datagrams or errors arrive.
    fn datagram(&self, domain: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno>;

    /// Publishing: listens on `host` with `backlog` and hands each connection to `sink`, taking
    /// a connection off the host listener only while [`InboundSink::poll_room`] says the guest
    /// has room for it, as a full Linux accept queue leaves new connections unanswered
    /// (`tcp_v4_syn_recv_sock` checks `sk_acceptq_is_full`). The returned handle stops
    /// publishing when dropped. Failure (a taken host port) is the host's problem: the network
    /// logs it and never shows it to the guest.
    fn publish(
        &self,
        host: SocketAddr,
        backlog: u32,
        sink: Arc<dyn InboundSink>,
    ) -> Result<Box<dyn Publication>, std::io::Error>;
}

/// Where a backend stream's connection stands, as far as the socket layer cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamState {
    /// The connection attempt is under way (`SYN_SENT`).
    Connecting,
    /// Connected; either side may have shut down a direction since.
    Established,
    /// `tcp_done`: the connection failed or was reset. `take_error` says why until taken.
    Closed,
}

/// A stream through the backend, non-blocking like a guest socket. `poll` reports what
/// `tcp_poll` would for the guest socket it carries; it registers nothing, since the stream
/// wakes the guest socket's queue (see [`Backend`]).
pub trait BackendStream: Pollable {
    /// Where the connection stands.
    fn state(&self) -> StreamState;
    /// `EAGAIN` when nothing is buffered; 0 at end of stream.
    fn read(&self, buf: &mut [u8], peek: bool) -> Result<usize, Errno>;
    /// `EAGAIN` when nothing fits.
    fn write(&self, data: &[u8]) -> Result<usize, Errno>;
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno>;
    /// The asynchronous error (a refused connection, a reset), cleared by the call.
    fn take_error(&self) -> Option<Errno>;
    /// The real peer, which `getpeername` reports.
    fn peer_addr(&self) -> SocketAddr;
    /// Bytes buffered for reading.
    fn input_len(&self) -> usize;
    /// `SIOCOUTQ`: bytes written but not yet taken by the peer, as far as the backend knows.
    fn output_len(&self) -> usize {
        0
    }
    /// Applies a socket option that changes host behaviour (`TCP_NODELAY`, keepalive). The
    /// default ignores it: the guest-visible value lives in the network.
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        let _ = (level, name, value);
        Ok(())
    }
}

/// A datagram endpoint through the backend. `poll` reports `IN` while datagrams are queued,
/// `OUT` while a send would not block (`sock_writeable`) and `ERR` while an error is pending;
/// like a stream's, it registers nothing, and it wakes the guest socket's queue with `OUT` once
/// a send that failed with `EAGAIN` would go through.
pub trait BackendDatagram: Pollable {
    /// Sends one datagram; `EAGAIN` when the host would block.
    fn send_to(&self, data: &[u8], to: SocketAddr) -> Result<usize, Errno>;
    /// `connect` (`Some`) or `connect(AF_UNSPEC)` (`None`) of the guest socket to a peer
    /// outside the namespace: a connected endpoint only receives from its peer and learns of
    /// errors sending to it (an ICMP port unreachable becomes `ECONNREFUSED`).
    fn connect(&self, peer: Option<SocketAddr>) -> Result<(), Errno>;
    /// The pending asynchronous error (`sk_err`), cleared by the call.
    fn take_error(&self) -> Option<Errno>;
    /// Receives one datagram into `buf`: (bytes copied, full datagram length, sender).
    fn recv_from(&self, buf: &mut [u8], peek: bool) -> Result<(usize, usize, SocketAddr), Errno>;
    /// The next datagram's size, 0 if none.
    fn input_len(&self) -> usize;
}

/// Whether a guest listener can take a connection now (see [`InboundSink::poll_room`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Room {
    /// A connection offered now goes into the accept queue.
    Ready,
    /// The accept queue is full. The table, if given, was registered on a queue woken once
    /// there is room again or the listener goes.
    Full,
    /// No guest listener holds the address: a connection would be refused.
    Gone,
}

/// Receives the connections a `Backend` accepted for a published port.
pub trait InboundSink: Send + Sync {
    /// Whether the guest listener for the published address could take a connection, as
    /// `poll` reports readiness: `table` registers on the listener's room queue first, so a
    /// backend that sees `Full` hears when to try again. Called from the backend's thread.
    fn poll_room(&self, table: Option<&mut PollTable<'_>>) -> Room;

    /// Called from the backend's thread. `wq` is the queue `stream` wakes; the guest socket
    /// made for the connection adopts it as its own. Returns `false` if the guest listener is
    /// gone or its queue filled since `poll_room`; the backend then drops the connection.
    fn incoming(&self, stream: Arc<dyn BackendStream>, wq: WaitQueue) -> bool;
}

/// A running publication; dropping it stops it.
pub trait Publication: Send + Sync {
    /// The host address actually bound (port 0 resolved).
    fn host_addr(&self) -> SocketAddr;

    /// The guest listener's `listen` backlog changed: the host listener follows it.
    fn set_backlog(&self, backlog: u32) {
        let _ = backlog;
    }
}
