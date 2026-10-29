//! `AF_NETLINK` sockets with `NETLINK_ROUTE` and `NETLINK_SOCK_DIAG`. Route sockets carry the
//! links, addresses and routes of the socket's network namespace, read (`getifaddrs`, `if_nameindex`, `ip link`, `ip addr`, `ip route`) and
//! changed (`ip link add/set/del`, `ip addr add/del`, `ip route add/del`); [`rtnl`] answers the
//! requests. Requests are parsed as `netlink_rcv_skb` and `rtnetlink_rcv_msg` do, and replies
//! are queued when the request is sent, so a non-blocking `recv` right after the `send` finds
//! them, as on Linux. The queue is bounded by `SO_RCVBUF`: a reply that finds it full is
//! dropped and the next receive fails with `ENOBUFS` (`netlink_overrun`). A classic BPF
//! filter the socket has attached (`SO_ATTACH_FILTER`) runs over each reply first, as
//! `netlink_unicast` and `netlink_dump` run `sk_filter`: it sees the netlink messages with no
//! device, and may drop the reply or cut it short. Diag sockets dump the namespace's IP and
//! Unix sockets for `ss` ([`diag`]).
//!
//! Not yet: multicast groups (no change is announced), neighbours, rules, traffic control and
//! namespace ids (`EOPNOTSUPP`, after the `CAP_NET_ADMIN` check every change makes), and other
//! netlink families.

mod diag;
mod rtnl;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{
    AddressFamily, MsgFlags, OpenFlags, SiCode, SocketFlags, SocketOption, SocketType,
};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysResult};
use wasixv2_vfs::Cap;
use wasixv2_vnet::bpf::SockFilter;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

pub(crate) use rtnl::{NoNamespaces, Sender};

use crate::{
    Kernel,
    cred::Cred,
    fasync::{Fasync, SockAsync, SockWake},
    file::{Backing, FileObject, Io, OpenFile},
    ns::NetNs,
    socket::{Blocking, RCV, Timeouts, WaitOn},
    sync::MutexExt,
};

const NETLINK_ROUTE: i32 = 0;
const NETLINK_SOCK_DIAG: i32 = 4;
/// `NLMSG_HDRLEN` and `NLMSG_ALIGNTO`.
const NLMSG_HDR: usize = 16;
const NLMSG_ALIGNTO: usize = 4;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
/// Types below this are control messages, never requests.
const NLMSG_MIN_TYPE: u16 = 0x10;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_MULTI: u16 = 2;
const NLM_F_ACK: u16 = 4;
const NLM_F_DUMP: u16 = 0x300;
/// An ack without the request's payload.
const NLM_F_CAPPED: u16 = 0x100;
const RTM_BASE: u16 = 16;
const RTM_MAX: u16 = 127;
/// `sizeof(struct sockaddr_nl)` and `sizeof(struct rtattr)`.
const SOCKADDR_NL: usize = 12;
const RTA_HDR: usize = 4;
/// `SO_SNDBUF` and `SO_RCVBUF`'s default.
const BUF_DEFAULT: usize = 212_992;
/// What a queued reply is charged on top of its bytes, roughly an skb's overhead.
const REPLY_OVERHEAD: usize = 768;
/// What `netlink_sendmsg` keeps of `SO_SNDBUF` for the skb: a request may be at most
/// `sndbuf - 32` bytes.
const REQUEST_OVERHEAD: usize = 32;

#[derive(Default)]
struct Queue {
    replies: VecDeque<Vec<u8>>,
    /// `sk_rmem_alloc`.
    queued: usize,
    /// `sk_err`: `ENOBUFS` after an overrun.
    err: Option<Errno>,
    sndbuf: usize,
    rcvbuf: usize,
    /// `dst_portid` and `dst_group` after `connect`.
    dst: (u32, u32),
}

pub(crate) struct NetlinkSocket {
    /// `NETLINK_ROUTE` or `NETLINK_SOCK_DIAG`.
    protocol: i32,
    queue: Mutex<Queue>,
    wait: WaitQueue,
    /// The files that get I/O signals.
    fasync: SockAsync,
    /// The credentials the socket was opened with (`file->f_cred`): a change asks
    /// `CAP_NET_ADMIN` of them besides of the sender (`file_ns_capable` in
    /// `__netlink_ns_capable`).
    pub(crate) opener: Arc<Cred>,
    /// The network namespace the socket was made in (`sock_net`).
    pub(crate) net: Arc<NetNs>,
    port: u32,
    pub(crate) timeouts: Timeouts,
    host: Arc<dyn Host>,
    /// `SO_ATTACH_FILTER`'s program and `SO_LOCK_FILTER`.
    filter: Mutex<SockFilter>,
}

/// One request's header.
struct Header {
    len: usize,
    kind: u16,
    flags: u16,
    seq: u32,
}

impl NetlinkSocket {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock_or_recover()
    }

    /// The largest request a send may carry; one more byte is copied to tell it is too long.
    pub(crate) fn max_request(&self) -> usize {
        self.queue().sndbuf.saturating_sub(REQUEST_OVERHEAD)
    }

    /// `netlink_sendmsg` to the kernel: handles each request in `data` (`netlink_rcv_skb`)
    /// and queues the replies. `net_admin` is the sender's own `CAP_NET_ADMIN`; a change also
    /// needs the opener's (`netlink_net_capable`). `sender` names other namespaces.
    pub(crate) fn send(
        &self,
        data: &[u8],
        net_admin: bool,
        sender: &dyn Sender,
    ) -> Result<usize, Errno> {
        let net_admin = net_admin && self.opener.ns_capable(self.net.user_ns(), Cap::NetAdmin);
        if data.len() > self.max_request() {
            return Err(Errno::EMSGSIZE);
        }
        let mut rest = data;
        while rest.len() >= NLMSG_HDR {
            let header = Header {
                len: u32::from_le_bytes(rest[0..4].try_into().unwrap()) as usize,
                kind: u16::from_le_bytes([rest[4], rest[5]]),
                flags: u16::from_le_bytes([rest[6], rest[7]]),
                seq: u32::from_le_bytes(rest[8..12].try_into().unwrap()),
            };
            if header.len < NLMSG_HDR || header.len > rest.len() {
                break;
            }
            let msg = &rest[..header.len];
            let err = match header.flags & NLM_F_REQUEST != 0 && header.kind >= NLMSG_MIN_TYPE {
                true if self.protocol == NETLINK_SOCK_DIAG => {
                    self.diag_request(&header, &msg[NLMSG_HDR..], net_admin, sender)
                }
                true => self.request(&header, &msg[NLMSG_HDR..], net_admin, sender),
                false => None,
            };
            if header.flags & NLM_F_ACK != 0 || err.is_some() {
                self.reply(ack(msg, header.seq, self.port, err));
            }
            // `NLMSG_ALIGN`ed, but never past the end.
            let step = header.len.next_multiple_of(NLMSG_ALIGNTO).min(rest.len());
            rest = &rest[step..];
        }
        self.data_ready();
        Ok(data.len())
    }

    /// `rtnetlink_rcv_msg` for one request with its payload; queues any reply and returns the
    /// error to acknowledge.
    fn request(
        &self,
        h: &Header,
        payload: &[u8],
        net_admin: bool,
        sender: &dyn Sender,
    ) -> Option<Errno> {
        use rtnl::*;
        if h.kind > RTM_MAX {
            return Some(Errno::EOPNOTSUPP);
        }
        // Every rtnetlink request starts with at least a family byte.
        payload.first()?;
        let get = (h.kind - RTM_BASE) & 3 == 2;
        if !get && !net_admin {
            return Some(Errno::EPERM);
        }
        let r = Request {
            net: &self.net,
            seq: h.seq,
            port: self.port,
            flags: h.flags,
            sender,
        };
        let dump = h.flags & NLM_F_DUMP != 0;
        let answer = match h.kind {
            RTM_GETLINK => get_link(&r, payload, dump),
            RTM_GETADDR => get_addr(&r, payload, dump),
            RTM_GETROUTE => get_route(&r, payload, dump),
            RTM_GETNEIGH => get_neigh(&r, dump),
            RTM_GETNSID => get_nsid(&r, payload, dump),
            RTM_GETRULE => get_rule(&r, payload, dump),
            RTM_NEWNSID => new_nsid(&r, payload).map(|()| Vec::new()),
            RTM_NEWLINK => new_link(&r, payload, false).map(|()| Vec::new()),
            RTM_SETLINK => new_link(&r, payload, true).map(|()| Vec::new()),
            RTM_DELLINK => del_link(&r, payload).map(|()| Vec::new()),
            RTM_NEWADDR => new_addr(&r, payload).map(|()| Vec::new()),
            RTM_DELADDR => del_addr(&r, payload).map(|()| Vec::new()),
            RTM_NEWROUTE => new_route(&r, payload).map(|()| Vec::new()),
            RTM_DELROUTE => del_route(&r, payload).map(|()| Vec::new()),
            _ => Err(Errno::EOPNOTSUPP),
        };
        match answer {
            Ok(reply) if reply.is_empty() => None,
            Ok(reply) => {
                self.reply(reply);
                None
            }
            Err(e) => Some(e),
        }
    }

    /// `sock_diag_rcv_msg` for one request; queues any reply and returns the error to
    /// acknowledge. Reading needs no capability; `net_admin` adds marks to the messages.
    fn diag_request(
        &self,
        h: &Header,
        payload: &[u8],
        net_admin: bool,
        sender: &dyn Sender,
    ) -> Option<Errno> {
        let r = rtnl::Request {
            net: &self.net,
            seq: h.seq,
            port: self.port,
            flags: h.flags,
            sender,
        };
        match diag::request(&r, h.kind, payload, net_admin) {
            Ok(reply) => {
                self.reply(reply);
                None
            }
            Err(e) => Some(e),
        }
    }

    /// The interfaces of the socket's namespace as they are now.
    #[cfg(test)]
    fn interfaces(&self) -> Vec<wasixv2_vnet::Interface> {
        rtnl::interfaces_of(&self.net)
    }

    /// `netlink_unicast` (and `netlink_dump`) to this socket: what the socket's filter keeps
    /// of the reply (`sk_filter`), queued as [`Self::queue_datagram`] queues it.
    fn reply(&self, mut reply: Vec<u8>) {
        match self.filter.lock_or_recover().trim_message(&reply) {
            Some(len) => reply.truncate(len),
            None => return,
        }
        self.queue_datagram(reply);
    }

    /// `__netlink_sendskb` after `netlink_attachskb`: queued unless the queue is over
    /// `SO_RCVBUF`, which drops the datagram and leaves `ENOBUFS` for the next receive
    /// (`netlink_overrun`).
    fn queue_datagram(&self, reply: Vec<u8>) {
        let mut q = self.queue();
        if q.queued > q.rcvbuf {
            if q.err.is_none() {
                q.err = Some(Errno::ENOBUFS);
                drop(q);
                // `sk_error_report`.
                self.wait.wake(PollMask::ERR);
                self.fasync.wake(SockWake::Io, SiCode::POLL_ERR);
            }
            return;
        }
        q.queued += reply.len() + REPLY_OVERHEAD;
        q.replies.push_back(reply);
    }

    /// One reply, waiting for one unless non-blocking; the whole datagram, which the caller
    /// truncates to the guest's buffer.
    pub(crate) async fn recv(
        &self,
        wait: &mut Blocking<'_>,
        flags: MsgFlags,
    ) -> SysResult<Vec<u8>> {
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        let take = || {
            let mut q = self.queue();
            if let Some(err) = q.err.take() {
                return Err(err);
            }
            if peek {
                return q.replies.front().cloned().ok_or(Errno::EAGAIN);
            }
            let reply = q.replies.pop_front().ok_or(Errno::EAGAIN)?;
            q.queued -= reply.len() + REPLY_OVERHEAD;
            Ok(reply)
        };
        wait.wait(WaitOn::Queue(&self.wait), PollMask::READABLE, take)
            .await
    }

    /// The blocking of one call in direction `dir`.
    pub(crate) fn blocking<'a>(&'a self, io: Io<'a>, dir: usize, flags: MsgFlags) -> Blocking<'a> {
        Blocking::new(&*self.host, io, flags, self.timeouts.get(dir))
    }

    /// `netlink_sendskb`: queues a datagram the kernel sends on its own, such as the cookie of
    /// a `SIGEV_THREAD` message queue notification, which no filter sees.
    pub(crate) fn deliver(&self, datagram: Vec<u8>) {
        self.queue_datagram(datagram);
        self.data_ready();
    }

    /// `sk_data_ready` (`sock_def_readable`).
    fn data_ready(&self) {
        self.wait.wake(PollMask::READABLE);
        self.fasync.wake(SockWake::WaitData, SiCode::POLL_IN);
    }

    pub(crate) fn bind(&self, raw: &[u8]) -> Result<i32, Errno> {
        if raw.len() < SOCKADDR_NL {
            return Err(Errno::EINVAL);
        }
        Ok(0)
    }

    pub(crate) fn local_addr(&self) -> Vec<u8> {
        sockaddr_nl(self.port)
    }

    /// `netlink_getname(peer = 1)`: where `connect` pointed the socket, the kernel by default.
    pub(crate) fn peer_addr(&self) -> Vec<u8> {
        let (pid, group) = self.queue().dst;
        let mut raw = sockaddr_nl(pid);
        // `netlink_group_mask`.
        let groups = match group {
            0 => 0,
            g => 1u32 << (g - 1),
        };
        raw[8..12].copy_from_slice(&groups.to_le_bytes());
        raw
    }

    /// `netlink_connect`. Pointing a socket at a process or a multicast group needs
    /// `CAP_NET_ADMIN` for rtnetlink, which has no `NL_CFG_F_NONROOT_SEND`.
    // TODO: messages to another netlink socket; sends always reach the kernel.
    pub(crate) fn connect(&self, raw: &[u8], net_admin: bool) -> Result<(), Errno> {
        let family = match raw.get(..2) {
            Some(f) => u16::from_le_bytes([f[0], f[1]]) as i32,
            None => return Err(Errno::EINVAL),
        };
        if family == AddressFamily::AF_UNSPEC.0 {
            self.queue().dst = (0, 0);
            return Ok(());
        }
        if family != AddressFamily::AF_NETLINK.0 || raw.len() < SOCKADDR_NL {
            return Err(Errno::EINVAL);
        }
        let pid = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        let groups = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        if (pid != 0 || groups != 0) && !net_admin {
            return Err(Errno::EPERM);
        }
        // `ffs(nl_groups)`: the lowest group named.
        let group = match groups {
            0 => 0,
            g => g.trailing_zeros() + 1,
        };
        self.queue().dst = (pid, group);
        Ok(())
    }

    pub(crate) fn getsockopt(&self, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        if SockFilter::handles(name) {
            let mut out = vec![0; wasixv2_vnet::bpf::MAX_PROGRAM_LEN];
            let n = self.filter.lock_or_recover().get(name, &mut out)?;
            out.truncate(n);
            return Ok(out);
        }
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(SocketType::SOCK_RAW.0),
            SocketOption::SO_PROTOCOL => int(self.protocol),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_NETLINK.0),
            SocketOption::SO_ERROR => int(self.queue().err.take().map_or(0, |e| e.0)),
            SocketOption::SO_SNDBUF => int(self.queue().sndbuf as i32),
            SocketOption::SO_RCVBUF => int(self.queue().rcvbuf as i32),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// The `SOL_SOCKET` options a netlink socket keeps, its filter's among them; others are
    /// accepted without effect.
    pub(crate) fn set_option(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
        if SockFilter::handles(name) {
            return self.filter.lock_or_recover().set(name, value);
        }
        let int = || {
            value
                .get(..4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
                .ok_or(Errno::EINVAL)
        };
        match SocketOption(name) {
            SocketOption::SO_SNDBUF | SocketOption::SO_SNDBUFFORCE => {
                self.queue().sndbuf = wasixv2_vnet::opts::sndbuf_from(int()?);
            }
            SocketOption::SO_RCVBUF | SocketOption::SO_RCVBUFFORCE => {
                self.queue().rcvbuf = wasixv2_vnet::opts::rcvbuf_from(int()?);
            }
            _ => {}
        }
        Ok(())
    }
}

/// The kernel's own netlink address, the source of every reply.
pub(crate) fn kernel_addr() -> Vec<u8> {
    sockaddr_nl(0)
}

fn sockaddr_nl(port: u32) -> Vec<u8> {
    let mut raw = vec![0; SOCKADDR_NL];
    raw[..2].copy_from_slice(&(AddressFamily::AF_NETLINK.0 as u16).to_le_bytes());
    raw[4..8].copy_from_slice(&port.to_le_bytes());
    raw
}

pub(crate) fn downcast(object: Arc<dyn FileObject>) -> Option<Arc<NetlinkSocket>> {
    let any: Arc<dyn std::any::Any + Send + Sync> = object;
    any.downcast().ok()
}

/// The `rtattr`s of a request (`nla_for_each_attr`): each `(type, payload)` as long as a whole
/// attribute fits.
fn attrs(mut raw: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        let len = u16::from_le_bytes([*raw.first()?, *raw.get(1)?]) as usize;
        let kind = u16::from_le_bytes([*raw.get(2)?, *raw.get(3)?]);
        if len < RTA_HDR || len > raw.len() {
            return None;
        }
        let data = &raw[RTA_HDR..len];
        let step = len.next_multiple_of(NLMSG_ALIGNTO).min(raw.len());
        raw = &raw[step..];
        Some((kind, data))
    })
}

fn nlmsg(kind: u16, flags: u16, seq: u32, port: u32, payload: &[u8]) -> Vec<u8> {
    let len = NLMSG_HDR + payload.len();
    let mut out = Vec::with_capacity(len.next_multiple_of(NLMSG_ALIGNTO));
    out.extend((len as u32).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(seq.to_le_bytes());
    out.extend(port.to_le_bytes());
    out.extend(payload);
    out.resize(len.next_multiple_of(NLMSG_ALIGNTO), 0);
    out
}

fn rta(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
    let len = RTA_HDR + data.len();
    out.extend((len as u16).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(data);
    out.resize(out.len().next_multiple_of(NLMSG_ALIGNTO), 0);
}

/// `netlink_ack`: `NLMSG_ERROR` with the error (0 for an ack) and the request, whole for an
/// error, its header alone (`NLM_F_CAPPED`) for an ack.
fn ack(request: &[u8], seq: u32, port: u32, err: Option<Errno>) -> Vec<u8> {
    let errno = err.map_or(0, |e| -e.0);
    let mut p = errno.to_le_bytes().to_vec();
    let flags = match err {
        Some(_) => {
            p.extend(request);
            0
        }
        None => {
            p.extend(&request[..NLMSG_HDR]);
            NLM_F_CAPPED
        }
    };
    nlmsg(NLMSG_ERROR, flags, seq, port, &p)
}

impl Pollable for NetlinkSocket {
    /// `datagram_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wait);
        let mut mask = PollMask::WRITABLE;
        let q = self.queue();
        if !q.replies.is_empty() {
            mask |= PollMask::READABLE;
        }
        if q.err.is_some() {
            mask |= PollMask::ERR;
        }
        mask
    }
}

impl FileObject for NetlinkSocket {
    fn messages(&self) -> bool {
        true
    }

    /// `sock_fasync`.
    fn fasync(&self, fasync: &Fasync<'_>) -> Option<Result<(), Errno>> {
        Some(self.fasync.fasync(fasync))
    }

    /// `sock_splice_read`.
    fn splice_read(&self) -> bool {
        true
    }

    /// `splice_to_socket`.
    fn splice_write(&self) -> bool {
        true
    }

    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.blocking(io, RCV, MsgFlags(0));
            let reply = self.recv(&mut wait, MsgFlags(0)).await?;
            let n = reply.len().min(buf.len());
            buf[..n].copy_from_slice(&reply[..n]);
            Ok(n)
        })
    }

    /// `write` has no credentials of the writer here, so a change rests on the opener's
    /// `CAP_NET_ADMIN` alone; `sendmsg` checks the sender's too.
    fn write<'a>(&'a self, _io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.send(data, true, &NoNamespaces)?) })
    }
}

impl Kernel {
    /// `netlink_create` for `NETLINK_ROUTE` and `NETLINK_SOCK_DIAG`.
    pub(crate) fn netlink_socket(
        &self,
        caller: &Caller<'_>,
        kind: SocketType,
        protocol: i32,
        flags: SocketFlags,
    ) -> SysResult<OpenFile> {
        if kind != SocketType::SOCK_RAW && kind != SocketType::SOCK_DGRAM {
            return Err(Errno::ESOCKTNOSUPPORT.into());
        }
        if protocol != NETLINK_ROUTE && protocol != NETLINK_SOCK_DIAG {
            return Err(Errno::EPROTONOSUPPORT.into());
        }
        let net = self.ns(caller).net.clone();
        let socket = NetlinkSocket {
            protocol,
            queue: Mutex::new(Queue {
                sndbuf: BUF_DEFAULT,
                rcvbuf: BUF_DEFAULT,
                ..Queue::default()
            }),
            wait: WaitQueue::new(),
            fasync: SockAsync::default(),
            opener: self.cred(caller),
            net,
            // `netlink_autobind`: the process's pid as it sees it.
            port: self.pids.own_nr(self.process(caller.task()).pid.0) as u32,
            timeouts: Timeouts::default(),
            host: self.host.clone(),
            filter: Mutex::new(SockFilter::default()),
        };
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(
            Backing::Stream(
                Arc::new(socket),
                self.pseudo.socket(self.cred(caller).fs_owner()),
            ),
            status,
        );
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use wasixv2_vnet::iface::flags::IFF_LOOPBACK;

    use super::{rtnl::*, *};

    /// A socket on a namespace with only `lo`, up, opened with `CAP_NET_ADMIN`.
    fn socket() -> NetlinkSocket {
        socket_in(net_ns())
    }

    /// A namespace with only `lo`, up.
    fn net_ns() -> Arc<NetNs> {
        let lo = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
        crate::ns::Ns::initial(
            crate::ns::NsKind::Net,
            Some(crate::ns::UserNs::initial_user()),
            crate::ns::Net {
                network: Some(lo),
                unix_names: Arc::default(),
                devconf: Default::default(),
                sysctl: Default::default(),
            },
        )
    }

    fn socket_in(net: Arc<NetNs>) -> NetlinkSocket {
        NetlinkSocket {
            protocol: NETLINK_ROUTE,
            queue: Mutex::new(Queue {
                sndbuf: BUF_DEFAULT,
                rcvbuf: BUF_DEFAULT,
                ..Queue::default()
            }),
            wait: WaitQueue::new(),
            fasync: SockAsync::default(),
            opener: Arc::new(Cred::new(
                &crate::Credentials::new(0, 0),
                net.user_ns().clone(),
            )),
            net,
            port: 7,
            timeouts: Timeouts::default(),
            host: Arc::new(crate::tests::TestHost),
            filter: Mutex::new(SockFilter::default()),
        }
    }

    fn header(len: u32, kind: u16, flags: u16) -> Vec<u8> {
        let mut h = len.to_le_bytes().to_vec();
        h.extend(kind.to_le_bytes());
        h.extend(flags.to_le_bytes());
        h.extend(9u32.to_le_bytes());
        h.extend(0u32.to_le_bytes());
        h
    }

    fn replies(nl: &NetlinkSocket) -> Vec<Vec<u8>> {
        nl.queue().replies.drain(..).collect()
    }

    #[test]
    fn malformed_requests_never_panic() {
        let nl = socket();
        // Unaligned lengths, lengths past the end, short headers, garbage of every size.
        for len in [0u32, 1, 15, 16, 17, 18, 19, 20, 21, 40, u32::MAX] {
            for total in [16usize, 17, 18, 19, 20, 21, 33, 64] {
                let mut data = header(len, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
                data.resize(total, 0xff);
                assert_eq!(nl.send(&data, true, &NoNamespaces), Ok(total));
            }
        }
        for seed in 0..2000u32 {
            let data: Vec<u8> = (0..(seed % 97))
                .map(|i| (seed.wrapping_mul(2654435761).wrapping_add(i * 40503) >> 7) as u8)
                .collect();
            let _ = nl.send(&data, true, &NoNamespaces);
        }
        // `nlmsg_len` 17 in a 17-byte send: handled, then the walk stops at the end.
        let mut data = header(17, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
        data.push(0);
        nl.queue().replies.clear();
        nl.queue().queued = 0;
        assert_eq!(nl.send(&data, true, &NoNamespaces), Ok(17));
        assert_eq!(replies(&nl).len(), 1);
    }

    #[test]
    fn requests_follow_netlink_rcv_skb() {
        let nl = socket();
        // Not a request: nothing, unless acked.
        let mut data = header(17, RTM_GETLINK, 0);
        data.push(0);
        nl.send(&data, true, &NoNamespaces).unwrap();
        assert!(replies(&nl).is_empty());
        let mut data = header(17, RTM_GETLINK, NLM_F_ACK);
        data.push(0);
        nl.send(&data, true, &NoNamespaces).unwrap();
        let acks = replies(&nl);
        assert_eq!(acks.len(), 1);
        assert_eq!(u16::from_le_bytes([acks[0][4], acks[0][5]]), NLMSG_ERROR);
        assert_eq!(i32::from_le_bytes(acks[0][16..20].try_into().unwrap()), 0);
        // A change needs CAP_NET_ADMIN, and then a truncated one is invalid.
        let mut data = header(17, RTM_NEWADDR, NLM_F_REQUEST);
        data.push(0);
        nl.send(&data, false, &NoNamespaces).unwrap();
        assert_eq!(error(&replies(&nl)[0]), -Errno::EPERM.0);
        nl.send(&data, true, &NoNamespaces).unwrap();
        assert_eq!(error(&replies(&nl)[0]), -Errno::EINVAL.0);
        // A request of a kind nothing answers.
        let mut data = header(17, 38 /* RTM_GETQDISC */, NLM_F_REQUEST);
        data.push(0);
        nl.send(&data, true, &NoNamespaces).unwrap();
        assert_eq!(error(&replies(&nl)[0]), -Errno::EOPNOTSUPP.0);
        // Non-dump RTM_GETLINK by index and by name.
        let mut data = header(32, RTM_GETLINK, NLM_F_REQUEST);
        data.extend([0u8; 4]);
        data.extend(1i32.to_le_bytes());
        data.extend([0u8; 8]);
        nl.send(&data, true, &NoNamespaces).unwrap();
        let link = replies(&nl);
        assert_eq!(u16::from_le_bytes([link[0][4], link[0][5]]), RTM_NEWLINK);
        let mut data = header(0, RTM_GETLINK, NLM_F_REQUEST);
        data.extend([0u8; IFINFOMSG]);
        rta(&mut data, IFLA_IFNAME, b"lo\0");
        let len = data.len() as u32;
        data[..4].copy_from_slice(&len.to_le_bytes());
        nl.send(&data, true, &NoNamespaces).unwrap();
        assert_eq!(replies(&nl).len(), 1);
        data[NLMSG_HDR + IFINFOMSG + RTA_HDR] = b'x';
        nl.send(&data, true, &NoNamespaces).unwrap();
        let err = replies(&nl);
        assert_eq!(
            i32::from_le_bytes(err[0][16..20].try_into().unwrap()),
            -Errno::ENODEV.0
        );
    }

    /// The error of an `NLMSG_ERROR` reply.
    fn error(reply: &[u8]) -> i32 {
        assert_eq!(u16::from_le_bytes([reply[4], reply[5]]), NLMSG_ERROR);
        i32::from_le_bytes(reply[16..20].try_into().unwrap())
    }

    /// `RTM_NEWLINK` for interface `index` with `ifi_flags`, `ifi_change` and `attrs`.
    fn new_link(
        index: i32,
        flags: u32,
        change: u32,
        nl_flags: u16,
        attrs: &[(u16, &[u8])],
    ) -> Vec<u8> {
        let mut data = header(0, RTM_NEWLINK, NLM_F_REQUEST | NLM_F_ACK | nl_flags);
        data.extend([0u8; 4]);
        data.extend(index.to_le_bytes());
        data.extend(flags.to_le_bytes());
        data.extend(change.to_le_bytes());
        for &(kind, value) in attrs {
            rta(&mut data, kind, value);
        }
        let len = data.len() as u32;
        data[..4].copy_from_slice(&len.to_le_bytes());
        data
    }

    fn lo_flags(nl: &NetlinkSocket) -> u32 {
        nl.interfaces()[0].flags
    }

    /// `ip link set lo down` and `up`: `rtnl_newlink` changes the flags `ifi_change` names
    /// (all of them when it is 0), and a dump right after shows the change.
    #[test]
    fn new_link_changes_the_flags_it_names() {
        use wasixv2_vnet::iface::flags::{IFF_LOWER_UP, IFF_RUNNING, IFF_UP};
        let nl = socket();
        let up = IFF_UP | IFF_RUNNING | IFF_LOWER_UP;
        assert_eq!(lo_flags(&nl), IFF_LOOPBACK | up);

        nl.send(&new_link(1, 0, IFF_UP, 0, &[]), false, &NoNamespaces)
            .unwrap();
        assert_eq!(error(&replies(&nl)[0]), -Errno::EPERM.0);
        nl.send(&new_link(1, 0, IFF_UP, 0, &[]), true, &NoNamespaces)
            .unwrap();
        assert_eq!(error(&replies(&nl)[0]), 0);
        assert_eq!(lo_flags(&nl), IFF_LOOPBACK);
        let mut dump = header(17, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
        dump.push(0);
        nl.send(&dump, true, &NoNamespaces).unwrap();
        let link = &replies(&nl)[0];
        let ifi_flags = u32::from_le_bytes(link[NLMSG_HDR + 8..NLMSG_HDR + 12].try_into().unwrap());
        assert_eq!(ifi_flags, IFF_LOOPBACK);

        // By name, with every flag at once.
        let by_name = new_link(0, IFF_UP, 0, 0, &[(IFLA_IFNAME, b"lo\0")]);
        nl.send(&by_name, true, &NoNamespaces).unwrap();
        assert_eq!(error(&replies(&nl)[0]), 0);
        assert_eq!(lo_flags(&nl), IFF_LOOPBACK | up);
    }

    /// `rtnl_newlink`'s answers for what it cannot change, in its order.
    #[test]
    fn new_link_refuses_what_it_cannot_do() {
        let nl = socket();
        let refused = |data: Vec<u8>| {
            nl.send(&data, true, &NoNamespaces).unwrap();
            error(&replies(&nl)[0])
        };
        assert_eq!(refused(new_link(-1, 1, 1, 0, &[])), -Errno::EINVAL.0);
        assert_eq!(refused(new_link(9, 1, 1, 0, &[])), -Errno::ENODEV.0);
        assert_eq!(
            refused(new_link(0, 1, 1, 0, &[(IFLA_IFNAME, b"eth9\0")])),
            -Errno::ENODEV.0
        );
        assert_eq!(
            refused(new_link(0, 1, 1, NLM_F_CREATE, &[(IFLA_IFNAME, b"eth9\0")])),
            -Errno::EOPNOTSUPP.0
        );
        assert_eq!(
            refused(new_link(1, 1, 1, NLM_F_EXCL, &[])),
            -Errno::EEXIST.0
        );
        assert_eq!(
            refused(new_link(1, 1, 1, NLM_F_REPLACE, &[])),
            -Errno::EOPNOTSUPP.0
        );
        // What no interface here has, such as a broadcast address to set.
        assert_eq!(
            refused(new_link(1, 1, 1, 0, &[(IFLA_BROADCAST, &[0xff; 6])])),
            -Errno::EOPNOTSUPP.0
        );
        // `dev_change_name` refuses an interface that is up.
        assert_eq!(
            refused(new_link(1, 1, 1, 0, &[(IFLA_IFNAME, b"lo0\0")])),
            -Errno::EBUSY.0
        );
        let mtu = 9000u32.to_le_bytes();
        assert_eq!(refused(new_link(1, 1, 1, 0, &[(IFLA_MTU, &mtu)])), 0);
        assert_eq!(nl.interfaces()[0].mtu, 9000);
        assert_eq!(refused(new_link(1, 1, 1, 0, &[(IFLA_IFNAME, b"lo\0")])), 0);
    }

    /// `BPF_RET | BPF_K` with `k`.
    fn ret(k: u32) -> Vec<u8> {
        [&0x06u16.to_le_bytes()[..], &[0, 0], &k.to_le_bytes()].concat()
    }

    /// An attached filter runs over each reply (`sk_filter` in `netlink_unicast`): 0 drops
    /// it, a smaller length cuts it short; what the kernel sends on its own
    /// (`netlink_sendskb`) passes. It reads back with `SO_GET_FILTER` and comes off with
    /// `SO_DETACH_FILTER`.
    #[test]
    fn attached_filters_drop_and_trim_replies() {
        let nl = socket();
        let mut getlink = header(32, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
        getlink.extend([0; 16]);
        let attach = SocketOption::SO_ATTACH_FILTER.0;
        nl.set_option(attach, &ret(0)).unwrap();
        assert_eq!(nl.getsockopt(attach).unwrap(), ret(0));
        nl.send(&getlink, true, &NoNamespaces).unwrap();
        assert!(replies(&nl).is_empty());
        nl.deliver(vec![1, 2, 3]);
        assert_eq!(replies(&nl), [vec![1, 2, 3]]);

        nl.set_option(attach, &ret(20)).unwrap();
        nl.send(&getlink, true, &NoNamespaces).unwrap();
        let got = replies(&nl);
        assert!(
            !got.is_empty() && got.iter().all(|r| r.len() == 20),
            "{got:?}"
        );

        nl.set_option(SocketOption::SO_DETACH_FILTER.0, &0i32.to_le_bytes())
            .unwrap();
        assert!(nl.getsockopt(attach).unwrap().is_empty());
        nl.send(&getlink, true, &NoNamespaces).unwrap();
        assert!(replies(&nl).iter().any(|r| r.len() > 20));
    }

    #[test]
    fn a_full_receive_queue_overruns_with_enobufs() {
        let nl = socket();
        let mut data = Vec::new();
        for _ in 0..4096 {
            let mut h = header(17, RTM_GETADDR, NLM_F_REQUEST | NLM_F_DUMP);
            h.extend([0, 0, 0, 0]);
            data.extend(h);
        }
        let mut sent = 0;
        while sent < 64 {
            nl.send(&data[..16 * 1024], true, &NoNamespaces).unwrap();
            sent += 1;
        }
        let q = nl.queue();
        assert!(q.queued <= q.rcvbuf + 4096, "bounded by SO_RCVBUF");
        assert_eq!(q.err, Some(Errno::ENOBUFS));
        drop(q);
        assert!(nl.poll(None).contains(PollMask::ERR));
        assert_eq!(
            nl.send(&vec![0; BUF_DEFAULT], true, &NoNamespaces),
            Err(Errno::EMSGSIZE)
        );
    }

    /// A request of `kind` with `flags`, a fixed header `body` and attributes.
    fn msg(kind: u16, flags: u16, body: &[u8], attrs: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut data = header(0, kind, NLM_F_REQUEST | flags);
        data.extend(body);
        for (kind, value) in attrs {
            rta(&mut data, *kind, value);
        }
        let len = data.len() as u32;
        data[..4].copy_from_slice(&len.to_le_bytes());
        data
    }

    /// Sends `data` and returns the replies, each split into messages.
    fn exchange(nl: &NetlinkSocket, data: &[u8], sender: &dyn Sender) -> Vec<Vec<u8>> {
        nl.send(data, true, sender).unwrap();
        let mut out = Vec::new();
        for reply in replies(nl) {
            let mut rest = &reply[..];
            while rest.len() >= NLMSG_HDR {
                let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
                out.push(rest[..len].to_vec());
                rest = &rest[len.next_multiple_of(NLMSG_ALIGNTO).min(rest.len())..];
            }
        }
        out
    }

    /// The acknowledged errno of a change sent with `NLM_F_ACK`.
    fn change(nl: &NetlinkSocket, data: &[u8], sender: &dyn Sender) -> i32 {
        error(&exchange(nl, data, sender)[0])
    }

    /// The attribute `kind` of a reply whose fixed header is `fixed` bytes.
    fn attr(reply: &[u8], fixed: usize, kind: u16) -> Option<Vec<u8>> {
        attrs(&reply[NLMSG_HDR + fixed..])
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.to_vec())
    }

    fn ifinfo(index: i32, flags: u32, change: u32) -> Vec<u8> {
        let mut body = vec![0u8; 4];
        body.extend(index.to_le_bytes());
        body.extend(flags.to_le_bytes());
        body.extend(change.to_le_bytes());
        body
    }

    fn name(n: &str) -> Vec<u8> {
        let mut v = n.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// Hands out one namespace for every pid.
    struct To(Arc<NetNs>);

    impl Sender for To {
        fn net_of_pid(&self, _: u32) -> Result<Arc<NetNs>, Errno> {
            Ok(self.0.clone())
        }

        fn net_of_fd(&self, _: i32) -> Result<Arc<NetNs>, Errno> {
            Err(Errno::EBADF)
        }

        fn net_capable(&self, _: &NetNs, _: Cap) -> bool {
            true
        }
    }

    /// Names one namespace, over whose owner the sender lacks `CAP_NET_ADMIN`.
    struct Refused(Arc<NetNs>);

    impl Sender for Refused {
        fn net_of_pid(&self, _: u32) -> Result<Arc<NetNs>, Errno> {
            Ok(self.0.clone())
        }

        fn net_of_fd(&self, _: i32) -> Result<Arc<NetNs>, Errno> {
            Err(Errno::EBADF)
        }

        fn net_capable(&self, _: &NetNs, _: Cap) -> bool {
            false
        }
    }

    /// A link goes to, or a veth's peer is made in, another namespace only with
    /// `CAP_NET_ADMIN` over its owner (`rtnl_link_get_net_capable`, `rtnl_get_peer_net`):
    /// `EPERM`, and nothing changes.
    #[test]
    fn links_go_only_to_namespaces_the_sender_administers() {
        let nl = socket();
        let other = net_ns();
        let there = socket_in(other.clone());
        let refused = Refused(other);
        let create = NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL;
        let mut info = Vec::new();
        rta(&mut info, IFLA_INFO_KIND, b"dummy\0");
        let add = msg(
            RTM_NEWLINK,
            create,
            &ifinfo(0, 0, 0),
            &[
                (IFLA_IFNAME, name("dummy0")),
                (IFLA_LINKINFO | 0x8000, info),
            ],
        );
        assert_eq!(change(&nl, &add, &refused), 0);
        let index = nl.interfaces()[1].index;
        let mov = msg(
            RTM_NEWLINK,
            NLM_F_ACK,
            &ifinfo(index as i32, 0, u32::MAX),
            &[(IFLA_NET_NS_PID, 42u32.to_le_bytes().to_vec())],
        );
        assert_eq!(change(&nl, &mov, &refused), -Errno::EPERM.0);
        assert_eq!(nl.interfaces().len(), 2, "dummy0 stays");

        let mut peer = ifinfo(0, 0, 0);
        rta(&mut peer, IFLA_IFNAME, &name("veth1"));
        rta(&mut peer, IFLA_NET_NS_PID, &42u32.to_le_bytes());
        let mut data = Vec::new();
        rta(&mut data, VETH_INFO_PEER, &peer);
        let mut info = Vec::new();
        rta(&mut info, IFLA_INFO_KIND, b"veth\0");
        rta(&mut info, IFLA_INFO_DATA | 0x8000, &data);
        let add = msg(
            RTM_NEWLINK,
            create,
            &ifinfo(0, 0, 0),
            &[(IFLA_IFNAME, name("veth0")), (IFLA_LINKINFO | 0x8000, info)],
        );
        assert_eq!(change(&nl, &add, &refused), -Errno::EPERM.0);
        assert_eq!(nl.interfaces().len(), 2);
        assert_eq!(there.interfaces().len(), 1, "no veth1 there");
    }

    /// `ip link add veth0 type veth peer name veth1`, then `ip link set veth1 netns <pid>`
    /// (`IFLA_NET_NS_PID`, as `tst_ns_ifmove` sends it), then `ip link del veth0`.
    #[test]
    fn veth_pairs_are_made_moved_and_deleted() {
        let nl = socket();
        let other = net_ns();
        let there = socket_in(other.clone());
        let to = To(other);
        let mut peer = ifinfo(0, 0, 0);
        rta(&mut peer, IFLA_IFNAME, &name("veth1"));
        let mut data = Vec::new();
        rta(&mut data, VETH_INFO_PEER, &peer);
        let mut info = Vec::new();
        rta(&mut info, IFLA_INFO_KIND, b"veth\0");
        rta(&mut info, IFLA_INFO_DATA | 0x8000, &data);
        let create = NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL;
        let add = msg(
            RTM_NEWLINK,
            create,
            &ifinfo(0, 0, 0),
            &[(IFLA_IFNAME, name("veth0")), (IFLA_LINKINFO | 0x8000, info)],
        );
        assert_eq!(change(&nl, &add, &to), 0);
        assert_eq!(change(&nl, &add, &to), -Errno::EEXIST.0);
        let names: Vec<_> = nl
            .interfaces()
            .into_iter()
            .map(|i| (i.index, i.name))
            .collect();
        assert_eq!(
            names,
            [(1, "lo".into()), (2, "veth1".into()), (3, "veth0".into())]
        );
        let dump = header(17, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
        let links = exchange(&nl, &[dump.clone(), vec![0]].concat(), &to);
        assert_eq!(
            attr(&links[2], IFINFOMSG, IFLA_LINK),
            Some(2u32.to_le_bytes().to_vec())
        );
        let kind = attr(&links[2], IFINFOMSG, IFLA_LINKINFO).unwrap();
        assert_eq!(
            attrs(&kind).next().unwrap(),
            (IFLA_INFO_KIND, &b"veth\0"[..])
        );

        let mov = msg(
            RTM_NEWLINK,
            NLM_F_ACK,
            &ifinfo(2, 0, u32::MAX),
            &[(IFLA_NET_NS_PID, 42u32.to_le_bytes().to_vec())],
        );
        assert_eq!(change(&nl, &mov, &to), 0);
        assert_eq!(nl.interfaces().len(), 2);
        assert_eq!(there.interfaces()[1].name, "veth1");
        let links = exchange(&nl, &[dump, vec![0]].concat(), &to);
        assert_eq!(
            attr(&links[1], IFINFOMSG, IFLA_LINK_NETNSID),
            Some(0i32.to_le_bytes().to_vec())
        );
        // `RTM_GETNSID` finds the id the dump gave the namespace; `RTM_NEWNSID` cannot give
        // it another.
        let nsid = |kind, flags| {
            let mut body = vec![0u8; 4];
            rta(&mut body, 2 /* NETNSA_PID */, &42u32.to_le_bytes());
            rta(&mut body, 1 /* NETNSA_NSID */, &5i32.to_le_bytes());
            let mut data = header(0, kind, NLM_F_REQUEST | flags);
            data.extend(body);
            let len = data.len() as u32;
            data[..4].copy_from_slice(&len.to_le_bytes());
            data
        };
        let got = exchange(&nl, &nsid(RTM_GETNSID, 0), &to);
        assert_eq!(attr(&got[0], 4, 1), Some(0i32.to_le_bytes().to_vec()));
        assert_eq!(
            change(&nl, &nsid(RTM_NEWNSID, NLM_F_ACK), &to),
            -Errno::EEXIST.0
        );
        // A namespace without an id gets the one asked for; a dump lists both.
        let third = To(net_ns());
        assert_eq!(change(&nl, &nsid(RTM_NEWNSID, NLM_F_ACK), &third), 0);
        let mut dump = header(20, RTM_GETNSID, NLM_F_REQUEST | NLM_F_DUMP);
        dump.extend([0; 4]);
        let ids = exchange(&nl, &dump, &to);
        let ids: Vec<_> = ids[..2].iter().map(|m| attr(m, 4, 1).unwrap()).collect();
        assert_eq!(ids, [0i32.to_le_bytes(), 5i32.to_le_bytes()]);
        // `lo` stays home; deleting it is not a request's.
        let mov_lo = msg(
            RTM_NEWLINK,
            NLM_F_ACK,
            &ifinfo(1, 0, 0),
            &[(IFLA_NET_NS_PID, 42u32.to_le_bytes().to_vec())],
        );
        assert_eq!(change(&nl, &mov_lo, &to), -Errno::EINVAL.0);
        let del = |index| msg(RTM_DELLINK, NLM_F_ACK, &ifinfo(index, 0, 0), &[]);
        assert_eq!(change(&nl, &del(1), &to), -Errno::EOPNOTSUPP.0);
        assert_eq!(change(&nl, &del(3), &to), 0);
        assert_eq!(there.interfaces().len(), 1);
        assert_eq!(change(&nl, &del(0), &to), -Errno::EINVAL.0);
    }

    fn ifaddr(family: i32, prefix: u8, index: u32) -> Vec<u8> {
        let mut body = vec![family as u8, prefix, 0, 0];
        body.extend(index.to_le_bytes());
        body
    }

    fn rtmsg(family: i32, dst_len: u8, scope: u8, kind: u8) -> Vec<u8> {
        let mut body = vec![
            family as u8,
            dst_len,
            0,
            0,
            wasixv2_vnet::iface::rt::RT_TABLE_MAIN as u8,
            3,
            scope,
            kind,
        ];
        body.extend(0u32.to_le_bytes());
        body
    }

    /// `ip addr add/del` and `ip route add/get/del` on a dummy interface, and the dumps
    /// `ip addr show` and `ip route show` read.
    #[test]
    fn addresses_and_routes_change_through_netlink() {
        let nl = socket();
        let us = NoNamespaces;
        let mut info = Vec::new();
        rta(&mut info, IFLA_INFO_KIND, b"dummy\0");
        let up = wasixv2_vnet::iface::flags::IFF_UP;
        let add = msg(
            RTM_NEWLINK,
            NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            &ifinfo(0, up, up),
            &[(IFLA_IFNAME, name("dummy0")), (IFLA_LINKINFO, info)],
        );
        assert_eq!(change(&nl, &add, &us), 0);
        let dummy = nl.interfaces()[1].clone();
        assert_ne!(dummy.flags & up, 0);
        let inet = AddressFamily::AF_INET.0;
        let a = [10u8, 0, 0, 1].to_vec();
        let addr = |kind, flags| {
            msg(
                kind,
                NLM_F_ACK | flags,
                &ifaddr(inet, 24, dummy.index),
                &[(IFA_LOCAL, a.clone()), (IFA_ADDRESS, a.clone())],
            )
        };
        let create = NLM_F_CREATE | NLM_F_EXCL;
        assert_eq!(change(&nl, &addr(RTM_NEWADDR, create), &us), 0);
        assert_eq!(
            change(&nl, &addr(RTM_NEWADDR, create), &us),
            -Errno::EEXIST.0
        );
        let mut dump = header(0, RTM_GETADDR, NLM_F_REQUEST | NLM_F_DUMP);
        dump.extend(ifaddr(inet, 0, 0));
        let len = dump.len() as u32;
        dump[..4].copy_from_slice(&len.to_le_bytes());
        let addrs = exchange(&nl, &dump, &us);
        // lo's 127.0.0.1 and ours, then NLMSG_DONE.
        assert_eq!(addrs.len(), 3);
        assert_eq!(attr(&addrs[1], 8, IFA_LOCAL), Some(a.clone()));

        let gw = [10u8, 0, 0, 254].to_vec();
        let route = |kind, gateway: &[u8]| {
            msg(
                kind,
                NLM_F_ACK | create,
                &rtmsg(inet, 0, 0, 1),
                &[(RTA_GATEWAY, gateway.to_vec())],
            )
        };
        assert_eq!(
            change(&nl, &route(RTM_NEWROUTE, &[172, 16, 0, 1]), &us),
            -Errno::ENETUNREACH.0
        );
        assert_eq!(change(&nl, &route(RTM_NEWROUTE, &gw), &us), 0);
        let mut dump = header(0, RTM_GETROUTE, NLM_F_REQUEST | NLM_F_DUMP);
        dump.extend(rtmsg(inet, 0, 0, 0));
        let len = dump.len() as u32;
        dump[..4].copy_from_slice(&len.to_le_bytes());
        let routes = exchange(&nl, &dump, &us);
        // The prefix route, the default route, NLMSG_DONE.
        assert_eq!(routes.len(), 3);
        assert_eq!(attr(&routes[1], 12, RTA_GATEWAY), Some(gw.clone()));
        let get = msg(
            RTM_GETROUTE,
            0,
            &rtmsg(inet, 32, 0, 0),
            &[(RTA_DST, vec![8, 8, 8, 8])],
        );
        let got = exchange(&nl, &get, &us);
        assert_eq!(u16::from_le_bytes([got[0][4], got[0][5]]), RTM_NEWROUTE);
        assert_eq!(
            attr(&got[0], 12, RTA_OIF),
            Some(dummy.index.to_le_bytes().to_vec())
        );
        let del = msg(
            RTM_DELROUTE,
            NLM_F_ACK,
            &rtmsg(inet, 0, wasixv2_vnet::iface::rt::RT_SCOPE_NOWHERE, 0),
            &[],
        );
        assert_eq!(change(&nl, &del, &us), 0);
        assert_eq!(change(&nl, &del, &us), -Errno::ESRCH.0);
        assert_eq!(error(&exchange(&nl, &get, &us)[0]), -Errno::ENETUNREACH.0);
        assert_eq!(change(&nl, &addr(RTM_DELADDR, 0), &us), 0);
        assert_eq!(
            change(&nl, &addr(RTM_DELADDR, 0), &us),
            -Errno::EADDRNOTAVAIL.0
        );
    }

    /// `ip rule`: Linux's default rules, the only ones; an IPv6 default route has no
    /// `RTA_DST` (`rt6_fill_node` puts one only with a prefix).
    #[test]
    fn the_default_rules_and_a_default_route_without_a_destination() {
        let nl = socket();
        let us = NoNamespaces;
        let mut dump = header(0, RTM_GETRULE, NLM_F_REQUEST | NLM_F_DUMP);
        dump.extend([0u8; 12]);
        let len = dump.len() as u32;
        dump[..4].copy_from_slice(&len.to_le_bytes());
        let rules = exchange(&nl, &dump, &us);
        // local, main and default for IPv4, local and main for IPv6, then NLMSG_DONE.
        assert_eq!(rules.len(), 6);
        assert_eq!(attr(&rules[0], 12, 15), Some(255u32.to_le_bytes().to_vec()));
        assert_eq!(
            attr(&rules[1], 12, 6),
            Some(32766u32.to_le_bytes().to_vec())
        );

        let mut info = Vec::new();
        rta(&mut info, IFLA_INFO_KIND, b"dummy\0");
        let up = wasixv2_vnet::iface::flags::IFF_UP;
        let add = msg(
            RTM_NEWLINK,
            NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            &ifinfo(0, up, up),
            &[(IFLA_IFNAME, name("dummy0")), (IFLA_LINKINFO, info)],
        );
        assert_eq!(change(&nl, &add, &us), 0);
        let index = nl.interfaces()[1].index as i32;
        let inet6 = AddressFamily::AF_INET6.0;
        let route = msg(
            RTM_NEWROUTE,
            NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            &rtmsg(inet6, 0, 0, 1),
            &[(RTA_OIF, index.to_le_bytes().to_vec())],
        );
        assert_eq!(change(&nl, &route, &us), 0);
        let mut dump = header(0, RTM_GETROUTE, NLM_F_REQUEST | NLM_F_DUMP);
        dump.extend(rtmsg(inet6, 0, 0, 0));
        let len = dump.len() as u32;
        dump[..4].copy_from_slice(&len.to_le_bytes());
        let routes = exchange(&nl, &dump, &us);
        let default = routes
            .iter()
            .find(|m| m.get(NLMSG_HDR + 1) == Some(&0))
            .unwrap();
        assert_eq!(attr(default, 12, RTA_DST), None);
    }

    /// The messages of a reply: (type, payload).
    fn messages(reply: &[u8]) -> Vec<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let mut rest = reply;
        while rest.len() >= NLMSG_HDR {
            let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
            let kind = u16::from_le_bytes([rest[4], rest[5]]);
            out.push((kind, rest[NLMSG_HDR..len].to_vec()));
            rest = &rest[len.next_multiple_of(NLMSG_ALIGNTO).min(rest.len())..];
        }
        out
    }

    fn diag_socket(net: Arc<NetNs>) -> NetlinkSocket {
        NetlinkSocket {
            protocol: NETLINK_SOCK_DIAG,
            ..socket_in(net)
        }
    }

    /// A `SOCK_DIAG_BY_FAMILY` dump request for `family` and `protocol` with `states`, and
    /// `extra` after the request (attributes).
    fn inet_dump(family: u8, protocol: u8, states: u32, extra: &[u8]) -> Vec<u8> {
        let mut req = vec![family, protocol, 0, 0];
        req.extend(states.to_le_bytes());
        req.extend([0u8; 48]);
        req.extend(extra);
        let mut data = header((16 + req.len()) as u32, 20, NLM_F_REQUEST | NLM_F_DUMP);
        data.extend(req);
        data
    }

    #[test]
    fn sock_diag_dumps_tcp_listeners_and_connections_from_the_port_table() {
        use wasixv2_vnet::{Domain, SocketKind};
        let net = net_ns();
        let network = net.network.clone().unwrap();
        let listener = network
            .socket(Domain::Inet, SocketKind::Stream, 0, 1000)
            .unwrap();
        listener
            .bind("127.0.0.1:8080".parse().unwrap(), false)
            .unwrap();
        listener.listen(5).unwrap();
        listener.graft(77);
        let client = network
            .socket(Domain::Inet, SocketKind::Stream, 0, 1000)
            .unwrap();
        let _ = client.connect("127.0.0.1:8080".parse().unwrap());
        // A bound UDP socket, which a TCP dump leaves out.
        let udp = network
            .socket(Domain::Inet, SocketKind::Datagram, 0, 0)
            .unwrap();
        udp.bind("127.0.0.1:5353".parse().unwrap(), false).unwrap();

        let nl = diag_socket(net.clone());
        let all = !0u32;
        nl.send(&inet_dump(2, 6, all, &[]), false, &NoNamespaces)
            .unwrap();
        let reply = replies(&nl).concat();
        let msgs = messages(&reply);
        assert_eq!(msgs.last().unwrap().0, NLMSG_DONE);
        let socks: Vec<_> = msgs[..msgs.len() - 1]
            .iter()
            .map(|(_, p)| p.clone())
            .collect();
        // The listener and both ends of the connection.
        assert_eq!(socks.len(), 3, "{msgs:?}");
        let listen = socks.iter().find(|p| p[1] == 10).expect("a listener");
        assert_eq!(listen[0], 2);
        assert_eq!(&listen[4..6], &8080u16.to_be_bytes());
        assert_eq!(&listen[8..12], &[127, 0, 0, 1]);
        // rqueue, wqueue (the backlog), uid, inode.
        let word = |p: &[u8], at: usize| u32::from_le_bytes(p[at..at + 4].try_into().unwrap());
        assert_eq!(word(listen, 56), 1, "one connection queued");
        assert_eq!(word(listen, 60), 5);
        assert_eq!(word(listen, 64), 1000);
        assert_eq!(word(listen, 68), 77);
        assert!(socks.iter().filter(|p| p[1] == 1).count() == 2);

        // Only listeners; then a port filter in bytecode: `sport = :8080`.
        nl.send(&inet_dump(2, 6, 1 << 10, &[]), false, &NoNamespaces)
            .unwrap();
        assert_eq!(messages(&replies(&nl).concat()).len(), 2);
        let mut bc = vec![11, 8, 12, 0, 0, 0];
        bc.extend(8080u16.to_le_bytes());
        let mut attr = ((4 + bc.len()) as u16).to_le_bytes().to_vec();
        attr.extend(1u16.to_le_bytes());
        attr.extend(&bc);
        nl.send(&inet_dump(2, 6, all, &attr), false, &NoNamespaces)
            .unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(msgs.len(), 3, "the listener, the accepted end, NLMSG_DONE");
        // A bad program is EINVAL; an unknown protocol ENOENT; a short request EINVAL.
        let mut bad = attr.clone();
        bad[4] = 99;
        nl.send(&inet_dump(2, 6, all, &bad), false, &NoNamespaces)
            .unwrap();
        let errno = |nl: &NetlinkSocket| {
            let msgs = messages(&replies(nl).concat());
            assert_eq!(msgs[0].0, NLMSG_ERROR);
            i32::from_le_bytes(msgs[0].1[..4].try_into().unwrap())
        };
        assert_eq!(errno(&nl), -Errno::EINVAL.0);
        nl.send(&inet_dump(2, 33, all, &[]), false, &NoNamespaces)
            .unwrap();
        assert_eq!(errno(&nl), -Errno::ENOENT.0);
        let mut short = header(20, 20, NLM_F_REQUEST | NLM_F_DUMP);
        short.extend([2, 6, 0, 0]);
        nl.send(&short, false, &NoNamespaces).unwrap();
        assert_eq!(errno(&nl), -Errno::EINVAL.0);
        // UDP lists its bound socket as unconnected (`TCP_CLOSE`).
        nl.send(&inet_dump(2, 17, all, &[]), false, &NoNamespaces)
            .unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].1[1], 7);
        drop((listener, client, udp));
    }

    #[test]
    fn sock_diag_dumps_unix_sockets_with_names_peers_and_queues() {
        use crate::unix::{Kind, PeerCred, Sock};
        let net = net_ns();
        let host: Arc<dyn Host> = Arc::new(crate::tests::TestHost);
        let a = Sock::new(Kind::Stream, net.clone(), PeerCred::default(), host.clone());
        let b = Sock::new(Kind::Stream, net.clone(), PeerCred::default(), host.clone());
        Sock::pair(&a, &b);
        a.graft(500);
        b.graft(501);
        // An unnamed socket without a file is never listed.
        let _hidden = Sock::new(Kind::Dgram, net.clone(), PeerCred::default(), host);

        let nl = diag_socket(net);
        // AF_UNIX, every state, UDIAG_SHOW_PEER | UDIAG_SHOW_RQLEN.
        let mut req = vec![1u8, 0, 0, 0];
        req.extend((!0u32).to_le_bytes());
        req.extend(0u32.to_le_bytes());
        req.extend(0x14u32.to_le_bytes());
        req.extend([0xffu8; 8]);
        let mut data = header(16 + 24, 20, NLM_F_REQUEST | NLM_F_DUMP);
        data.extend(&req);
        nl.send(&data, false, &NoNamespaces).unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(msgs.len(), 3, "{msgs:?}");
        let first = msgs
            .iter()
            .find(|(_, p)| p[4..8] == 500u32.to_le_bytes())
            .unwrap();
        assert_eq!(first.1[0], 1);
        assert_eq!(first.1[1], 1, "SOCK_STREAM");
        assert_eq!(first.1[2], 1, "TCP_ESTABLISHED");
        let attrs: Vec<_> = attrs(&first.1[16..])
            .map(|(k, d)| (k, d.to_vec()))
            .collect();
        assert!(
            attrs.contains(&(2, 501u32.to_le_bytes().to_vec())),
            "{attrs:?}"
        );
        assert!(attrs.iter().any(|(k, _)| *k == 4));
        assert!(attrs.contains(&(6, vec![0])), "UNIX_DIAG_SHUTDOWN always");
        // The socket of one inode, without NLM_F_DUMP; none for an unknown one.
        let mut exact = req.clone();
        exact[8..12].copy_from_slice(&501u32.to_le_bytes());
        let mut data = header(16 + 24, 20, NLM_F_REQUEST);
        data.extend(&exact);
        nl.send(&data, false, &NoNamespaces).unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1[4..8], 501u32.to_le_bytes());
        exact[8..12].copy_from_slice(&9u32.to_le_bytes());
        let mut data = header(16 + 24, 20, NLM_F_REQUEST);
        data.extend(&exact);
        nl.send(&data, false, &NoNamespaces).unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(
            i32::from_le_bytes(msgs[0].1[..4].try_into().unwrap()),
            -Errno::ENOENT.0
        );
        // Families without a handler.
        let mut data = header(16 + 24, 20, NLM_F_REQUEST | NLM_F_DUMP);
        let mut other = req.clone();
        other[0] = 17;
        data.extend(&other);
        nl.send(&data, false, &NoNamespaces).unwrap();
        let msgs = messages(&replies(&nl).concat());
        assert_eq!(
            i32::from_le_bytes(msgs[0].1[..4].try_into().unwrap()),
            -Errno::ENOENT.0
        );
    }
}
