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
//!
//! A wakeup whose events hold `PRI` says that a new urgent mark arrived (`sk_send_sigurg`, then
//! `sk_data_ready` once the urgent byte is in): the kernel sends `SIGURG` to the socket's owner
//! on it, so nothing else wakes a socket with `PRI`.

use std::{
    fmt,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Weak},
    time::Duration,
};

use wasixv2_abi::{Errno, MsgFlags, SoEeOrigin};
use wasixv2_wait::{PollTable, Pollable, WaitQueue};

use crate::{
    iface::{AddMode, AddrMatch, IfAddr, Route, RouteDecision, RouteMatch},
    net::{MovedLink, Net, Rtnl},
    ratelimit::IcmpLimit,
};

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
    /// `SOCK_RAW`
    Raw,
    /// `SOCK_SEQPACKET`: SCTP's one-to-many style.
    SeqPacket,
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
    /// Control messages from `sendmsg`, such as `IP_PKTINFO` or `IP_TTL`. The kernel has
    /// checked and removed the `SOL_SOCKET` ones (`__sock_cmsg_send`); the socket validates the
    /// rest as its protocol does and fails the send with `EINVAL` (or `EPERM`) as Linux would.
    pub control: &'a [Control],
    /// The caller holds `CAP_NET_RAW`, which some control messages need (IPv6 option headers).
    pub net_raw: bool,
    /// The kernel's monotonic clock (`CLOCK_MONOTONIC`) at the call: the time of whatever
    /// the send sets off, such as the ICMP messages a namespace answers with and their rate
    /// limits.
    pub now: Duration,
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

/// One entry of a socket's error queue (`sk_error_queue`): Linux's `sock_exterr_skb` and the
/// datagram it quotes. `recvmsg(MSG_ERRQUEUE)` returns the payload, `dst` as the address and
/// the rest as an `IP_RECVERR`/`IPV6_RECVERR` control message (`ip_recv_error`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedError {
    /// `ee_errno`.
    pub errno: Errno,
    /// `ee_origin`.
    pub origin: SoEeOrigin,
    /// `ee_type` and `ee_code`: the ICMP type and code for ICMP origins.
    pub type_: u8,
    pub code: u8,
    /// `ee_info`, such as the next-hop MTU of a "fragmentation needed".
    pub info: u32,
    pub data: u32,
    /// `SO_EE_OFFENDER`: the node that reported an ICMP error.
    pub offender: Option<SocketAddr>,
    /// The quoted datagram's destination, which `recvmsg` reports as the sender; `None` for
    /// origins that quote no datagram.
    pub dst: Option<SocketAddr>,
    /// The quoted datagram's payload, as far as the error quotes it.
    pub payload: Vec<u8>,
}

impl QueuedError {
    /// `ip_local_error`, `ipv6_local_error`: an error the socket's own stack found, about a
    /// packet to `dst` (and the socket's `port`), with `info` (an MTU for `EMSGSIZE`).
    pub fn local(errno: Errno, info: u32, dst: IpAddr, port: u16) -> Self {
        Self {
            errno,
            origin: SoEeOrigin::SO_EE_ORIGIN_LOCAL,
            type_: 0,
            code: 0,
            info,
            data: 0,
            offender: None,
            dst: Some(SocketAddr::new(dst, port)),
            payload: Vec::new(),
        }
    }

    /// `is_icmp_err_skb`: an entry that also stands for the socket's pending error.
    pub fn is_icmp(&self) -> bool {
        self.origin == SoEeOrigin::SO_EE_ORIGIN_ICMP
            || self.origin == SoEeOrigin::SO_EE_ORIGIN_ICMP6
    }
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
    /// `sk_uid`, the owner the socket was made for (`sock_i_uid`).
    pub uid: u32,
    /// The inode of the socket's file (`sock_i_ino`), 0 while it has none: a connection still
    /// in the accept queue.
    pub inode: u64,
    /// What `tcp_diag_get_info` and `get_tcp4_sock` report as the receive and send queues: a
    /// listener's accept queue length and backlog, a connection's unread and unacknowledged
    /// bytes, a UDP socket's `sk_rmem_alloc` and 0.
    pub rx_queue: usize,
    pub tx_queue: usize,
    /// `IPV6_V6ONLY`.
    pub v6only: bool,
    /// `SOCK_BINDPORT_LOCK`: `bind` chose the port.
    pub port_locked: bool,
    /// `sk_bound_dev_if` (`SO_BINDTODEVICE`), 0 for none.
    pub bound_dev: u32,
}

/// One SCTP endpoint, for `/proc/net/sctp/eps` (`sctp_eps_seq_show`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SctpEndpoint {
    /// `sctp_sock.type`: 0 one-to-one (`SCTP_SOCKET_TCP`), 1 one-to-many, 2 peeled off.
    pub style: u8,
    /// `sk_state`, as the TCP state numbers SCTP's socket states share.
    pub state: u8,
    pub port: u16,
    /// The bound addresses, the wildcard as it was bound.
    pub addrs: Vec<IpAddr>,
    pub uid: u32,
    pub inode: u64,
}

/// One SCTP association, for `/proc/net/sctp/assocs` (`sctp_assocs_seq_show`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SctpAssocEntry {
    /// The owning socket's style and state, as in [`SctpEndpoint`].
    pub style: u8,
    pub sk_state: u8,
    /// The association's state, internal numbering (`SCTP_STATE_ESTABLISHED` is 3).
    pub state: u8,
    pub id: i32,
    /// `sndbuf_used`: bytes sent and not yet taken by the peer's window.
    pub tx_queue: usize,
    /// `rmem_alloc`: bytes queued for reading here.
    pub rx_queue: usize,
    pub uid: u32,
    pub inode: u64,
    pub lport: u16,
    pub rport: u16,
    /// The association's local addresses and the primary path's source among them.
    pub laddrs: Vec<IpAddr>,
    pub primary_src: Option<IpAddr>,
    /// The peer's addresses and the primary path among them.
    pub paddrs: Vec<IpAddr>,
    pub primary: Option<IpAddr>,
    /// `hbinterval` in milliseconds.
    pub hbinterval: u32,
    pub incnt: u16,
    pub outcnt: u16,
    pub max_retrans: u16,
    pub sndbuf: usize,
    pub rcvbuf: usize,
}

/// The sysctls that make a namespace ignore echo requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EchoIgnore {
    /// `net.ipv4.icmp_echo_ignore_all`, `net.ipv4.icmp_echo_ignore_broadcasts`.
    All,
    Broadcasts,
    /// `net.ipv6.icmp.echo_ignore_all`, `echo_ignore_multicast`. (`echo_ignore_anycast` has
    /// nothing to act on: no namespace has anycast addresses.)
    All6,
    Multicast6,
}

/// One ping or raw ICMP socket, for `/proc/net/{icmp,icmp6,raw,raw6}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcmpEntry {
    pub domain: Domain,
    /// A raw socket (`SOCK_RAW`), else a ping socket.
    pub raw: bool,
    /// `inet_rcv_saddr`, `None` for the wildcard.
    pub local: Option<IpAddr>,
    /// `inet_num`: a ping socket's identifier, a raw socket's protocol.
    pub num: u16,
    /// The connected peer and port (`inet_dport`).
    pub peer: Option<(IpAddr, u16)>,
    /// `sk_rmem_alloc`: what the receive queue is charged.
    pub rx_queue: usize,
    /// `sk_uid`.
    pub uid: u32,
    /// `sock_i_ino`, 0 while the socket has no file.
    pub inode: u64,
    /// `sk_drops`: messages lost to a full receive queue.
    pub drops: u32,
}

/// A network interface as netlink, `SIOCGIF*`, `getifaddrs` and `/proc/net` show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    /// The interface index, from 1.
    pub index: u32,
    pub name: String,
    /// `IFF_*` flags as `dev_get_flags` reports them (`IFF_UP`, `IFF_LOOPBACK`,
    /// `IFF_RUNNING`, ...).
    pub flags: u32,
    pub mtu: u32,
    pub mac: [u8; 6],
    /// `IFLA_INFO_KIND` (`"veth"`, `"dummy"`) for an interface a request made.
    pub kind: Option<&'static str>,
    /// A veth end's peer (`IFLA_LINK`), with the id this namespace knows the peer's namespace
    /// by (`IFLA_LINK_NETNSID`) when it is another one.
    pub peer: Option<(u32, Option<i32>)>,
    /// Addresses, IPv4 first in Linux's order.
    pub addresses: Vec<IfAddr>,
    /// `net.ipv6.conf.<name>.disable_ipv6`.
    pub ipv6_disabled: bool,
    /// The multicast groups the interface is in, newest first, with how many memberships
    /// hold each (see [`crate::iface::Link::groups`]).
    pub groups: Vec<(IpAddr, u32)>,
    /// The sources the memberships' filters name, per group in `groups`' order (see
    /// [`crate::iface::Link::group_sources`]).
    pub sources: Vec<GroupSources>,
}

/// One group's sources on an interface (`ip_mc_list`'s and `ifmcaddr6`'s source list): each
/// source and how many memberships include and exclude it (`sf_count[MCAST_INCLUDE]`,
/// `sf_count[MCAST_EXCLUDE]`), in the order they were first named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSources {
    pub group: IpAddr,
    pub sources: Vec<(IpAddr, u32, u32)>,
}

/// What `RTM_NEWLINK` with `NLM_F_CREATE` makes (`rtnl_newlink_create`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewLink {
    /// The name, or a pattern with `%d`; `None` takes the kind's (`veth%d`, `dummy%d`).
    pub name: Option<String>,
    pub mtu: Option<u32>,
    pub mac: Option<[u8; 6]>,
    pub kind: NewLinkKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewLinkKind {
    Dummy,
    /// A pair: the new interface and its peer (`VETH_INFO_PEER`), both in this namespace.
    Veth {
        peer_name: Option<String>,
        peer_mtu: Option<u32>,
        peer_mac: Option<[u8; 6]>,
    },
}

/// A network namespace: what the kernel's `socket` and the network pseudo files use.
pub trait Network: Send + Sync {
    /// Linux's `inet_create`/`inet6_create`: a new, unbound socket. `protocol` is the guest's
    /// third `socket` argument: 0 picks the default; an unsupported value is
    /// `EPROTONOSUPPORT`, an unsupported kind/protocol pair `ESOCKTNOSUPPORT` or
    /// `EPROTONOSUPPORT` as Linux decides. The kernel has checked what creating the socket
    /// needs: `CAP_NET_RAW` for `SOCK_RAW`, `ping_group_range` for an ICMP datagram (ping)
    /// socket. `uid` owns the socket
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

    /// The namespace's bound ping sockets and its raw ICMP sockets, each newest first, as
    /// their hash chains have them.
    fn icmp_sockets(&self) -> Vec<IcmpEntry> {
        Vec::new()
    }

    /// The namespace's SCTP endpoints and their associations, for `/proc/net/sctp`.
    fn sctp_endpoints(&self) -> Vec<SctpEndpoint> {
        Vec::new()
    }

    fn sctp_assocs(&self) -> Vec<SctpAssocEntry> {
        Vec::new()
    }

    /// `/proc/net/sctp/snmp`: the namespace's SCTP counters, named and ordered as
    /// `sctp_snmp_list`.
    fn sctp_snmp(&self) -> Vec<(&'static str, u64)> {
        Vec::new()
    }

    /// The vsock g2h transport of the backend behind this namespace ([`Backend::vsock_cid`]).
    fn vsock_cid(&self) -> Option<u32> {
        None
    }

    /// [`Backend::vsock_connect`] through the backend behind this namespace; `ENODEV` without
    /// one.
    fn vsock_connect(
        &self,
        cid: u32,
        port: u32,
        wq: WaitQueue,
    ) -> Result<Arc<dyn BackendStream>, Errno> {
        let _ = (cid, port, wq);
        Err(Errno::ENODEV)
    }

    /// Host-to-guest stream listener (`virtio_transport_recv_listen`). The handle closes
    /// the host listener when dropped. No inbound transport by default.
    fn vsock_listen(
        &self,
        port: u32,
        backlog: u32,
        sink: Arc<dyn VsockInboundSink>,
    ) -> Result<Box<dyn VsockPublication>, Errno> {
        let _ = (port, backlog, sink);
        Err(Errno::EOPNOTSUPP)
    }

    /// The namespace's interfaces; a fresh namespace has only loopback.
    fn interfaces(&self) -> Vec<Interface> {
        Vec::new()
    }

    /// `dev_change_flags` on the interface `index` (`SIOCSIFFLAGS`, `RTM_NEWLINK`), with
    /// `flags` as `__dev_change_flags` takes them: `IFF_UP` brings it up or down. `ENODEV` for
    /// no such interface, `EOPNOTSUPP` from a network whose interfaces never change.
    fn change_flags(&self, index: u32, flags: u32) -> Result<(), Errno> {
        let _ = (index, flags);
        Err(Errno::EOPNOTSUPP)
    }

    /// This namespace as a [`Net`], which namespace ids name.
    fn net(&self) -> Option<Weak<Net>> {
        None
    }

    /// `peernet2id`: the id this namespace knows `other` by, if it gave it one; `None` for
    /// itself.
    fn peer_id(&self, other: &dyn Network) -> Option<i32> {
        let _ = other;
        None
    }

    /// `rtnl_net_newid` (`ip netns set`): gives `other` the id `id`, or the lowest free one.
    /// `EEXIST` if `other` has an id already or `id` is taken.
    fn assign_peer_id(&self, other: &dyn Network, id: Option<i32>) -> Result<i32, Errno> {
        let _ = (other, id);
        Err(Errno::EOPNOTSUPP)
    }

    /// The ids given to other namespaces, for `RTM_GETNSID` dumps.
    fn peer_ids(&self) -> Vec<i32> {
        Vec::new()
    }

    /// One of the sysctls the namespace's ICMP heeds answering echo requests.
    fn set_icmp_echo_ignore(&self, which: EchoIgnore, ignore: bool) -> Result<(), Errno> {
        let _ = (which, ignore);
        Err(Errno::EOPNOTSUPP)
    }

    /// One of the sysctls that rate-limit the ICMP messages the namespace sends.
    fn set_icmp_limit(&self, limit: IcmpLimit) -> Result<(), Errno> {
        let _ = limit;
        Err(Errno::EOPNOTSUPP)
    }

    /// `net.ipv6.conf.<name>.disable_ipv6` (`addrconf_disable_ipv6`).
    fn set_ipv6_disabled(&self, index: u32, disabled: bool) -> Result<(), Errno> {
        let _ = (index, disabled);
        Err(Errno::EOPNOTSUPP)
    }

    /// The routes of every table, for `RTM_GETROUTE` dumps and `/proc/net/route`.
    fn routes(&self) -> Vec<Route> {
        Vec::new()
    }

    /// The route lookup a socket sending to `dst` makes (`RTM_GETROUTE` for one address).
    fn route_get(&self, dst: IpAddr) -> Result<RouteDecision, Errno> {
        let _ = dst;
        Err(Errno::ENETUNREACH)
    }

    /// `RTM_NEWLINK` with `NLM_F_CREATE`: a dummy interface or a veth pair, down. Returns the
    /// new interface's index, and for a pair its peer's.
    fn new_link(&self, link: NewLink) -> Result<(u32, Option<u32>), Errno> {
        let _ = link;
        Err(Errno::EOPNOTSUPP)
    }

    /// `RTM_DELLINK`: deleting a veth end deletes its peer too. Interfaces no request made
    /// (`lo`, the uplink) are `EOPNOTSUPP`.
    fn del_link(&self, index: u32) -> Result<(), Errno> {
        let _ = index;
        Err(Errno::EOPNOTSUPP)
    }

    /// `dev_change_name` (`IFLA_IFNAME`, `SIOCSIFNAME`).
    fn rename_link(&self, index: u32, name: &str) -> Result<(), Errno> {
        let _ = (index, name);
        Err(Errno::EOPNOTSUPP)
    }

    /// `dev_set_mtu` (`IFLA_MTU`, `SIOCSIFMTU`).
    fn set_mtu(&self, index: u32, mtu: u32) -> Result<(), Errno> {
        let _ = (index, mtu);
        Err(Errno::EOPNOTSUPP)
    }

    /// `dev_set_mac_address` (`IFLA_ADDRESS`, `SIOCSIFHWADDR`).
    fn set_mac(&self, index: u32, mac: [u8; 6]) -> Result<(), Errno> {
        let _ = (index, mac);
        Err(Errno::EOPNOTSUPP)
    }

    /// `RTM_NEWADDR` (`ip addr add`, `SIOCSIFADDR`).
    fn add_address(&self, index: u32, addr: IfAddr, mode: AddMode) -> Result<(), Errno> {
        let _ = (index, addr, mode);
        Err(Errno::EOPNOTSUPP)
    }

    /// `RTM_DELADDR` (`ip addr del`, `SIOCDIFADDR`).
    fn del_address(&self, index: u32, which: &AddrMatch) -> Result<(), Errno> {
        let _ = (index, which);
        Err(Errno::EOPNOTSUPP)
    }

    /// `RTM_NEWROUTE` (`ip route add`, `SIOCADDRT`).
    fn add_route(&self, route: Route, mode: AddMode) -> Result<(), Errno> {
        let _ = (route, mode);
        Err(Errno::EOPNOTSUPP)
    }

    /// `RTM_DELROUTE` (`ip route del`, `SIOCDELRT`).
    fn del_route(&self, which: &RouteMatch) -> Result<(), Errno> {
        let _ = which;
        Err(Errno::EOPNOTSUPP)
    }

    /// The first half of moving interface `index` to another namespace
    /// (`__dev_change_net_namespace`): it goes down and leaves this namespace with its
    /// addresses and routes. `EINVAL` for an interface that stays in its namespace (`lo`, the
    /// uplink). See [`crate::move_link`].
    fn take_link(&self, rtnl: &Rtnl, index: u32) -> Result<MovedLink, Errno> {
        let _ = (rtnl, index);
        Err(Errno::EOPNOTSUPP)
    }

    /// The second half: the interface joins this namespace, keeping its index if free, and
    /// returns its index; on failure the link comes back.
    fn adopt_link(&self, rtnl: &Rtnl, link: MovedLink) -> Result<u32, (Errno, MovedLink)> {
        let _ = rtnl;
        Err((Errno::EOPNOTSUPP, link))
    }
}

/// An IP socket. The kernel wraps it as an open-file object; `poll` must report exactly what
/// Linux's `tcp_poll` and `udp_poll` would (a fresh stream socket is `OUT | WRNORM | HUP`, a
/// listener is `IN | RDNORM` while connections are queued, `RDHUP` after the peer shut down,
/// `ERR` while an error is pending, `PRI` while an urgent byte waits for `MSG_OOB`), and every
/// state change must wake the queues `poll` registered with the matching events.
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
    /// ignores the address. On a stream, `MSG_OOB` makes the last byte taken urgent.
    fn send(&self, msg: SendMsg<'_>) -> Result<usize, Errno>;

    /// Receives into `buf` without blocking. `flags` may hold `MSG_PEEK`, `MSG_TRUNC`,
    /// `MSG_OOB`, `MSG_ERRQUEUE`; the kernel handles `MSG_WAITALL` and `MSG_DONTWAIT`. Returns
    /// `len == 0` at end of stream; `EAGAIN` when nothing is queued; `ENOTCONN` for an
    /// unconnected stream. A stream read stops in front of an urgent byte and steps over it
    /// unless `SO_OOBINLINE` is set; `MSG_OOB` reads that byte alone, never waits, and fails
    /// as `tcp_recv_urg` does (`EINVAL` without one to read).
    fn recv(&self, buf: &mut [u8], flags: MsgFlags) -> Result<RecvMsg, Errno>;

    /// `getsockname`: the bound address, or the unspecified address and port 0 if unbound.
    fn local_addr(&self) -> Result<SocketAddr, Errno>;

    /// `getpeername`: `ENOTCONN` unless connected.
    fn peer_addr(&self) -> Result<SocketAddr, Errno>;

    /// `sock_error`: takes the pending asynchronous error (`SO_ERROR`), clearing it.
    fn take_error(&self) -> Option<Errno>;

    /// `SIOCINQ`/`FIONREAD`: bytes queued for a stream (up to an urgent byte not read inline),
    /// the next datagram's size for a datagram socket; `EINVAL` for a listener.
    fn input_len(&self) -> Result<usize, Errno>;

    /// `SIOCATMARK`: whether a stream's next byte is urgent. `ENOTTY` for protocols without
    /// urgent data.
    fn at_mark(&self) -> Result<bool, Errno> {
        Err(Errno::ENOTTY)
    }

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

    /// `sock_graft`: the kernel made the socket a file whose inode is `ino`, which the port
    /// table reports ([`PortEntry::inode`]).
    fn graft(&self, ino: u64) {
        let _ = ino;
    }

    /// For a protocol that keeps message boundaries on every socket type (SCTP): the largest
    /// message a send can take. The kernel then hands `send` the whole message (at most one
    /// byte more, so an oversized one fails as the protocol decides) and `recv` returns one
    /// message, or its next part when the buffer is short, never looping for `MSG_WAITALL`.
    /// `None` for byte streams and datagrams, which the kernel tells apart by `kind`.
    fn max_record(&self) -> Option<usize> {
        None
    }

    /// SCTP's own operations, which `setsockopt` and `getsockopt` multiplex.
    fn sctp(&self) -> Option<&dyn SctpOps> {
        None
    }
}

/// The SCTP operations behind `SCTP_SOCKOPT_BINDX_*`, `SCTP_SOCKOPT_CONNECTX*` and
/// `SCTP_SOCKOPT_PEELOFF*`, whose packed address arrays, descriptors and blocking the kernel
/// handles.
pub trait SctpOps {
    /// `sctp_bindx_add`/`sctp_bindx_rem`: adds the addresses to the socket's bind list (binding
    /// it first if unbound) or removes them, undoing what was done if one fails.
    /// `privileged` is `CAP_NET_BIND_SERVICE`, as for `Socket::bind`.
    fn bindx(&self, addrs: &[SocketAddr], add: bool, privileged: bool) -> Result<(), Errno>;

    /// `__sctp_connect` with every address of a peer endpoint (one port): sets up an association
    /// and returns its id. The association is up when this returns; the kernel reports
    /// `EINPROGRESS` to a non-blocking caller, as `sctp_wait_for_connect` does.
    fn connectx(&self, addrs: &[SocketAddr]) -> Result<i32, Errno>;

    /// `sctp_do_peeloff`: branches the association `id` of a one-to-many socket off into a
    /// socket of its own.
    fn peeloff(&self, id: i32) -> Result<Arc<dyn Socket>, Errno>;
}

/// A published UDP port: the endpoint bound to it and the handle that keeps it published.
pub type PublishedDatagram = (Arc<dyn BackendDatagram>, Box<dyn Publication>);

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

    /// An ICMP endpoint for echo requests to outside addresses, of the given family: a host
    /// ICMP datagram socket. `send_to` takes an echo request and the address (port 0), which
    /// the host may give an identifier of its own; `recv_from` returns the echo replies, ICMP
    /// header first. It wakes `wq` as [`Backend::datagram`]'s does. The default carries no
    /// ICMP (`EPERM`).
    fn ping(&self, domain: Domain, wq: WaitQueue) -> Result<Arc<dyn BackendDatagram>, Errno> {
        let _ = (domain, wq);
        Err(Errno::EPERM)
    }

    /// Publishing a UDP port: a datagram endpoint bound to `host`, so replies leave from the
    /// published port. It serves no guest socket until [`BackendDatagram::attach`]ed, and drops
    /// what arrives meanwhile, as Linux drops a datagram for a port no socket holds. The
    /// endpoint stays bound while it or the returned handle lives. Failure (a taken host port)
    /// is the host's problem, as for [`Backend::publish`].
    fn publish_datagram(&self, host: SocketAddr) -> Result<PublishedDatagram, std::io::Error> {
        let _ = host;
        Err(std::io::ErrorKind::Unsupported.into())
    }

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

    /// The context id the host gives this machine as a vsock guest: its g2h transport
    /// (virtio-vsock's guest side), or `None` for a backend that carries no `AF_VSOCK`. The
    /// kernel then has only its loopback transport, as a Linux host does.
    fn vsock_cid(&self) -> Option<u32> {
        None
    }

    /// Starts a vsock stream connection to `port` of the host-side context `cid`, as the g2h
    /// transport sends `VIRTIO_VSOCK_OP_REQUEST`: like [`Backend::connect`]'s stream, it
    /// reports `OUT` once the host accepted and is [`StreamState::Closed`] with `ECONNRESET`
    /// once the host refused (a reset), and wakes `wq` on every change. `ENODEV` without a
    /// g2h transport ([`Backend::vsock_cid`] is `None`).
    fn vsock_connect(
        &self,
        cid: u32,
        port: u32,
        wq: WaitQueue,
    ) -> Result<Arc<dyn BackendStream>, Errno> {
        let _ = (cid, port, wq);
        Err(Errno::ENODEV)
    }

    /// Host-to-guest stream listener (`virtio_transport_recv_listen`). The handle closes
    /// the host listener when dropped. No inbound transport by default.
    fn vsock_listen(
        &self,
        port: u32,
        backlog: u32,
        sink: Arc<dyn VsockInboundSink>,
    ) -> Result<Box<dyn VsockPublication>, Errno> {
        let _ = (port, backlog, sink);
        Err(Errno::EOPNOTSUPP)
    }

    /// Asks the host's resolver, for the namespace's DNS server (`NetConfig::dns_server`), and
    /// returns at once: `done` gets the answer later, from the backend's own thread as
    /// [`InboundSink::incoming`] is called, or before `lookup` returns when the backend knows
    /// it already, so the caller holds no lock a socket takes. The default has no resolver
    /// and fails every lookup.
    fn lookup(&self, lookup: Lookup, done: LookupDone) {
        let _ = lookup;
        done(Err(LookupError::Failed));
    }
}

/// A question the namespace's DNS server asks the backend's resolver ([`Backend::lookup`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Lookup {
    /// The addresses of a fully qualified name, written without the trailing dot, of both
    /// families, and its canonical name: `getaddrinfo` with `AI_CANONNAME`, never applying the
    /// host's search list.
    Name(String),
    /// The name of an address: `getnameinfo` with `NI_NAMEREQD`.
    Addr(IpAddr),
    /// A DNS query message, forwarded as it is to the host's name servers.
    Forward(Vec<u8>),
}

/// What a [`Lookup`] found, and for how many seconds that holds (the TTL of the records made
/// of it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub answer: Answer,
    pub ttl: u32,
}

/// The answer to a [`Lookup`] of the same kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The addresses of a name, possibly none (the name exists without addresses), and its
    /// canonical name.
    Addrs {
        canonical: String,
        addrs: Vec<IpAddr>,
    },
    /// The name of an address.
    Name(String),
    /// The name servers' response message, with the query's identifier.
    Message(Vec<u8>),
}

/// Why a [`Lookup`] found nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// The name or address has no entry (`NXDOMAIN`), for `ttl` seconds.
    NotFound { ttl: u32 },
    /// The host could not tell (`SERVFAIL`).
    Failed,
}

/// Receives the result of a [`Backend::lookup`].
pub type LookupDone = Box<dyn FnOnce(Result<Found, LookupError>) + Send>;

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
/// wakes the guest socket's queue (see [`Backend`]), with `PRI` for a new urgent mark.
///
/// Urgent data follows `tcp_recvmsg` and `tcp_recv_urg` as [`crate::RecvQueue`] keeps them,
/// with the guest's `SO_OOBINLINE`, which arrives through `set_option`.
pub trait BackendStream: Pollable {
    /// Where the connection stands.
    fn state(&self) -> StreamState;
    /// `EAGAIN` when nothing is buffered; 0 at end of stream.
    fn read(&self, buf: &mut [u8], peek: bool) -> Result<usize, Errno>;
    /// `EAGAIN` when nothing fits.
    fn write(&self, data: &[u8]) -> Result<usize, Errno>;
    /// [`BackendStream::write`] with `MSG_OOB`: the last byte taken is urgent.
    fn write_urgent(&self, data: &[u8]) -> Result<usize, Errno>;
    /// `tcp_recv_urg`: the urgent byte, left in place with `peek`.
    fn read_urgent(&self, peek: bool) -> Result<u8, Errno>;
    /// `SIOCATMARK`.
    fn at_mark(&self) -> bool;
    fn shutdown(&self, how: Shutdown) -> Result<(), Errno>;
    /// The asynchronous error (a refused connection, a reset), cleared by the call.
    fn take_error(&self) -> Option<Errno>;
    /// The real peer, which `getpeername` reports.
    fn peer_addr(&self) -> SocketAddr;
    /// Bytes buffered for reading, up to an urgent byte not read inline (`SIOCINQ`).
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
/// `OUT` while a send would not block (`sock_writeable`) and `ERR` while an error is pending
/// or the error queue holds entries (`datagram_poll`);
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
    /// `IP_RECVERR` or `IPV6_RECVERR` changed on the guest socket; `value` is the new value as
    /// a guest `int`, 0 or 1. While one is on, the errors the host reports wait in the
    /// endpoint's error queue for [`BackendDatagram::take_queued_error`] and become the pending
    /// error even when unconnected (`__udp4_lib_err`); turning one off empties the queue. The
    /// default ignores them, as it does every other option: guest-visible values live in the
    /// network.
    fn set_option(&self, level: i32, name: i32, value: &[u8]) -> Result<(), Errno> {
        let _ = (level, name, value);
        Ok(())
    }
    /// Takes the oldest entry of the error queue, addresses as the host saw them. As in
    /// `sock_dequeue_err_skb`, the pending error becomes the next entry's if that is an ICMP
    /// one, and none once the ICMP entries are gone.
    fn take_queued_error(&self) -> Option<QueuedError> {
        None
    }
    /// A published endpoint (see [`Backend::publish_datagram`]) serves the guest socket whose
    /// queue is `wq`, or with `None` no socket and drops what arrives. Either way it starts
    /// afresh: nothing queued, no pending error, unconnected and without `IP_RECVERR`. The
    /// guest socket keeps one queue for life, so the endpoint switches queues, never the
    /// socket. Other endpoints belong to one socket from the start: `EOPNOTSUPP`.
    fn attach(&self, wq: Option<WaitQueue>) -> Result<(), Errno> {
        let _ = wq;
        Err(Errno::EOPNOTSUPP)
    }
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

/// Receives host-to-guest vsock connections. Backpressure follows [`InboundSink`].
pub trait VsockInboundSink: Send + Sync {
    fn poll_room(&self, table: Option<&mut PollTable<'_>>) -> Room;
    fn incoming(&self, cid: u32, port: u32, stream: Arc<dyn BackendStream>, wq: WaitQueue) -> bool;
}

/// Keeps a host-to-guest vsock listener alive.
pub trait VsockPublication: Send + Sync {
    fn set_backlog(&self, backlog: u32);
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
