//! `AF_NETLINK` sockets with the `NETLINK_ROUTE` basics: `RTM_GETLINK` and `RTM_GETADDR` of
//! the network namespace's interface table, which is what `getifaddrs`, `if_nameindex` and
//! `ip link show dev` ask for. Requests are parsed as `netlink_rcv_skb` and
//! `rtnetlink_rcv_msg` do, and replies are queued when the request is sent, so a non-blocking
//! `recv` right after the `send` finds them, as on Linux. The queue is bounded by `SO_RCVBUF`:
//! a reply that finds it full is dropped and the next receive fails with `ENOBUFS`
//! (`netlink_overrun`).
//!
//! Not yet: routes (`RTM_GETROUTE`), multicast groups, changes (`RTM_NEWADDR` from the guest
//! is `EPERM`, as without `CAP_NET_ADMIN`), and other netlink families.

use std::{
    collections::VecDeque,
    net::IpAddr,
    sync::{Arc, Mutex, MutexGuard},
};

use wasixv2_abi::{AddressFamily, MsgFlags, OpenFlags, SocketFlags, SocketOption, SocketType};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysResult};
use wasixv2_vnet::Interface;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    ns::NetNs,
    socket::{Blocking, RCV, Timeouts, WaitOn},
};

const NETLINK_ROUTE: i32 = 0;
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
const RTM_NEWLINK: u16 = 16;
const RTM_GETLINK: u16 = 18;
const RTM_NEWADDR: u16 = 20;
const RTM_GETADDR: u16 = 22;
const IFLA_ADDRESS: u16 = 1;
const IFLA_BROADCAST: u16 = 2;
const IFLA_IFNAME: u16 = 3;
const IFLA_MTU: u16 = 4;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;
const IFA_LABEL: u16 = 3;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
const IFF_LOOPBACK: u32 = 0x8;
const RT_SCOPE_UNIVERSE: u8 = 0;
const RT_SCOPE_HOST: u8 = 254;
/// `sizeof(struct sockaddr_nl)`, `sizeof(struct ifinfomsg)` and `sizeof(struct rtattr)`.
const SOCKADDR_NL: usize = 12;
const IFINFOMSG: usize = 16;
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
    queue: Mutex<Queue>,
    wait: WaitQueue,
    /// The interfaces of `net` when the socket was made.
    interfaces: Vec<Interface>,
    /// The network namespace the socket was made in (`sock_net`).
    pub(crate) net: Arc<NetNs>,
    port: u32,
    pub(crate) timeouts: Timeouts,
    host: Arc<dyn Host>,
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
        self.queue.lock().unwrap()
    }

    /// The largest request a send may carry; one more byte is copied to tell it is too long.
    pub(crate) fn max_request(&self) -> usize {
        self.queue().sndbuf.saturating_sub(REQUEST_OVERHEAD)
    }

    /// `netlink_sendmsg` to the kernel: handles each request in `data` (`netlink_rcv_skb`)
    /// and queues the replies.
    pub(crate) fn send(&self, data: &[u8]) -> Result<usize, Errno> {
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
                true => self.request(&header, &msg[NLMSG_HDR..]),
                false => None,
            };
            if header.flags & NLM_F_ACK != 0 || err.is_some() {
                self.reply(ack(msg, header.seq, self.port, err));
            }
            // `NLMSG_ALIGN`ed, but never past the end.
            let step = header.len.next_multiple_of(NLMSG_ALIGNTO).min(rest.len());
            rest = &rest[step..];
        }
        self.wait.wake(PollMask::READABLE);
        Ok(data.len())
    }

    /// `rtnetlink_rcv_msg` for one request with its payload; queues any reply and returns the
    /// error to acknowledge.
    fn request(&self, h: &Header, payload: &[u8]) -> Option<Errno> {
        if h.kind > RTM_MAX {
            return Some(Errno::EOPNOTSUPP);
        }
        // Every rtnetlink request starts with at least a family byte.
        let &family = payload.first()?;
        let get = (h.kind - RTM_BASE) & 3 == 2;
        if !get {
            return Some(Errno::EPERM);
        }
        let dump = h.flags & NLM_F_DUMP != 0;
        match (h.kind, dump) {
            (RTM_GETLINK, true) => self.reply(self.dump(h.seq, |out| {
                for iface in &self.interfaces {
                    out.push(link_msg(iface, h.seq, self.port, NLM_F_MULTI));
                }
            })),
            (RTM_GETLINK, false) => match self.find_link(payload) {
                Ok(iface) => self.reply(link_msg(iface, h.seq, self.port, 0)),
                Err(errno) => return Some(errno),
            },
            (RTM_GETADDR, true) => self.reply(self.dump(h.seq, |out| {
                for iface in &self.interfaces {
                    for &(ip, prefix) in &iface.addresses {
                        let wanted = match family as i32 {
                            0 => true,
                            f if f == AddressFamily::AF_INET.0 => ip.is_ipv4(),
                            f if f == AddressFamily::AF_INET6.0 => ip.is_ipv6(),
                            _ => false,
                        };
                        if wanted {
                            out.push(addr_msg(iface, ip, prefix, h.seq, self.port));
                        }
                    }
                }
            })),
            _ => return Some(Errno::EOPNOTSUPP),
        }
        None
    }

    /// `rtnl_getlink`: the interface a request names by index or by `IFLA_IFNAME`.
    fn find_link(&self, payload: &[u8]) -> Result<&Interface, Errno> {
        let ifinfo = payload.get(..IFINFOMSG).ok_or(Errno::EINVAL)?;
        let index = i32::from_le_bytes(ifinfo[4..8].try_into().unwrap());
        let found = if index > 0 {
            self.interfaces.iter().find(|i| i.index as i32 == index)
        } else {
            let name = attrs(&payload[IFINFOMSG..])
                .find(|(kind, _)| *kind == IFLA_IFNAME)
                .map(|(_, data)| data.split(|&b| b == 0).next().unwrap_or(data))
                .ok_or(Errno::EINVAL)?;
            self.interfaces.iter().find(|i| i.name.as_bytes() == name)
        };
        found.ok_or(Errno::ENODEV)
    }

    fn dump(&self, seq: u32, fill: impl FnOnce(&mut Vec<Vec<u8>>)) -> Vec<u8> {
        let mut msgs = Vec::new();
        fill(&mut msgs);
        let mut out: Vec<u8> = msgs.concat();
        out.extend(nlmsg(
            NLMSG_DONE,
            NLM_F_MULTI,
            seq,
            self.port,
            &0i32.to_le_bytes(),
        ));
        out
    }

    /// `netlink_unicast` to this socket: queued unless the queue is over `SO_RCVBUF`, which
    /// drops the reply and leaves `ENOBUFS` for the next receive (`netlink_overrun`).
    fn reply(&self, reply: Vec<u8>) {
        let mut q = self.queue();
        if q.queued > q.rcvbuf {
            if q.err.is_none() {
                q.err = Some(Errno::ENOBUFS);
                drop(q);
                self.wait.wake(PollMask::ERR);
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
    /// a `SIGEV_THREAD` message queue notification.
    pub(crate) fn deliver(&self, datagram: Vec<u8>) {
        self.reply(datagram);
        self.wait.wake(PollMask::READABLE);
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
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(SocketType::SOCK_RAW.0),
            SocketOption::SO_PROTOCOL => int(NETLINK_ROUTE),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_NETLINK.0),
            SocketOption::SO_ERROR => int(self.queue().err.take().map_or(0, |e| e.0)),
            SocketOption::SO_SNDBUF => int(self.queue().sndbuf as i32),
            SocketOption::SO_RCVBUF => int(self.queue().rcvbuf as i32),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// The `SOL_SOCKET` options a netlink socket keeps; others are accepted without effect.
    pub(crate) fn set_option(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
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

/// `RTM_NEWLINK`: `struct ifinfomsg` plus name, MTU and hardware addresses.
fn link_msg(iface: &Interface, seq: u32, port: u32, flags: u16) -> Vec<u8> {
    let loopback = iface.flags & IFF_LOOPBACK != 0;
    let mut p = Vec::new();
    p.push(0); // ifi_family
    p.push(0);
    p.extend(
        match loopback {
            true => ARPHRD_LOOPBACK,
            false => ARPHRD_ETHER,
        }
        .to_le_bytes(),
    );
    p.extend((iface.index as i32).to_le_bytes());
    p.extend(iface.flags.to_le_bytes());
    p.extend(0u32.to_le_bytes()); // ifi_change
    let mut name = iface.name.as_bytes().to_vec();
    name.push(0);
    rta(&mut p, IFLA_IFNAME, &name);
    rta(&mut p, IFLA_MTU, &iface.mtu.to_le_bytes());
    // A made-up, locally administered MAC; zeros for loopback, as Linux reports.
    let mac = match loopback {
        true => [0; 6],
        false => [0x02, 0, 0, 0, 0, iface.index as u8],
    };
    rta(&mut p, IFLA_ADDRESS, &mac);
    let broadcast = match loopback {
        true => [0; 6],
        false => [0xff; 6],
    };
    rta(&mut p, IFLA_BROADCAST, &broadcast);
    nlmsg(RTM_NEWLINK, flags, seq, port, &p)
}

/// `RTM_NEWADDR`: `struct ifaddrmsg` plus the address (and local address and label for IPv4).
fn addr_msg(iface: &Interface, ip: IpAddr, prefix: u8, seq: u32, port: u32) -> Vec<u8> {
    let (family, bytes) = match ip {
        IpAddr::V4(v4) => (AddressFamily::AF_INET.0 as u8, v4.octets().to_vec()),
        IpAddr::V6(v6) => (AddressFamily::AF_INET6.0 as u8, v6.octets().to_vec()),
    };
    let scope = match ip.is_loopback() {
        true => RT_SCOPE_HOST,
        false => RT_SCOPE_UNIVERSE,
    };
    let mut p = vec![family, prefix, 0x80 /* IFA_F_PERMANENT */, scope];
    p.extend(iface.index.to_le_bytes());
    rta(&mut p, IFA_ADDRESS, &bytes);
    if ip.is_ipv4() {
        rta(&mut p, IFA_LOCAL, &bytes);
        let mut label = iface.name.as_bytes().to_vec();
        label.push(0);
        rta(&mut p, IFA_LABEL, &label);
    }
    nlmsg(RTM_NEWADDR, NLM_F_MULTI, seq, port, &p)
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

    /// `sock_splice_read`.
    fn splice_read(&self) -> bool {
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

    fn write<'a>(&'a self, _io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.send(data)?) })
    }
}

impl Kernel {
    /// `netlink_create` for `NETLINK_ROUTE`.
    pub(crate) fn netlink_socket(
        &self,
        caller: &Caller<'_>,
        kind: SocketType,
        protocol: i32,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        if kind != SocketType::SOCK_RAW && kind != SocketType::SOCK_DGRAM {
            return Err(Errno::ESOCKTNOSUPPORT.into());
        }
        if protocol != NETLINK_ROUTE {
            return Err(Errno::EPROTONOSUPPORT.into());
        }
        let net = self.ns(caller).net.clone();
        let interfaces = net
            .network
            .as_ref()
            .map(|network| network.interfaces())
            .unwrap_or_default();
        let socket = NetlinkSocket {
            queue: Mutex::new(Queue {
                sndbuf: BUF_DEFAULT,
                rcvbuf: BUF_DEFAULT,
                ..Queue::default()
            }),
            wait: WaitQueue::new(),
            interfaces,
            net,
            port: self.process(caller.task()).pid.0,
            timeouts: Timeouts::default(),
            host: self.host.clone(),
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
        let [fd] = self.install_files(caller, [file], flags.contains(SocketFlags::SOCK_CLOEXEC))?;
        Ok(fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket() -> NetlinkSocket {
        let lo = Interface {
            index: 1,
            name: "lo".into(),
            flags: IFF_LOOPBACK | 1,
            mtu: 65536,
            addresses: vec![("127.0.0.1".parse().unwrap(), 8)],
        };
        NetlinkSocket {
            queue: Mutex::new(Queue {
                sndbuf: BUF_DEFAULT,
                rcvbuf: BUF_DEFAULT,
                ..Queue::default()
            }),
            wait: WaitQueue::new(),
            interfaces: vec![lo],
            net: crate::ns::Ns::initial(
                crate::ns::NsKind::Net,
                None,
                crate::ns::Net {
                    network: None,
                    unix_names: Arc::default(),
                },
            ),
            port: 7,
            timeouts: Timeouts::default(),
            host: Arc::new(crate::tests::TestHost),
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
                assert_eq!(nl.send(&data), Ok(total));
            }
        }
        for seed in 0..2000u32 {
            let data: Vec<u8> = (0..(seed % 97))
                .map(|i| (seed.wrapping_mul(2654435761).wrapping_add(i * 40503) >> 7) as u8)
                .collect();
            let _ = nl.send(&data);
        }
        // `nlmsg_len` 17 in a 17-byte send: handled, then the walk stops at the end.
        let mut data = header(17, RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP);
        data.push(0);
        nl.queue().replies.clear();
        nl.queue().queued = 0;
        assert_eq!(nl.send(&data), Ok(17));
        assert_eq!(replies(&nl).len(), 1);
    }

    #[test]
    fn requests_follow_netlink_rcv_skb() {
        let nl = socket();
        // Not a request: nothing, unless acked.
        let mut data = header(17, RTM_GETLINK, 0);
        data.push(0);
        nl.send(&data).unwrap();
        assert!(replies(&nl).is_empty());
        let mut data = header(17, RTM_GETLINK, NLM_F_ACK);
        data.push(0);
        nl.send(&data).unwrap();
        let acks = replies(&nl);
        assert_eq!(acks.len(), 1);
        assert_eq!(u16::from_le_bytes([acks[0][4], acks[0][5]]), NLMSG_ERROR);
        assert_eq!(i32::from_le_bytes(acks[0][16..20].try_into().unwrap()), 0);
        // A change needs CAP_NET_ADMIN.
        let mut data = header(17, RTM_NEWADDR, NLM_F_REQUEST);
        data.push(0);
        nl.send(&data).unwrap();
        let err = replies(&nl);
        assert_eq!(
            i32::from_le_bytes(err[0][16..20].try_into().unwrap()),
            -Errno::EPERM.0
        );
        // Non-dump RTM_GETLINK by index and by name.
        let mut data = header(32, RTM_GETLINK, NLM_F_REQUEST);
        data.extend([0u8; 4]);
        data.extend(1i32.to_le_bytes());
        data.extend([0u8; 8]);
        nl.send(&data).unwrap();
        let link = replies(&nl);
        assert_eq!(u16::from_le_bytes([link[0][4], link[0][5]]), RTM_NEWLINK);
        let mut data = header(0, RTM_GETLINK, NLM_F_REQUEST);
        data.extend([0u8; IFINFOMSG]);
        rta(&mut data, IFLA_IFNAME, b"lo\0");
        let len = data.len() as u32;
        data[..4].copy_from_slice(&len.to_le_bytes());
        nl.send(&data).unwrap();
        assert_eq!(replies(&nl).len(), 1);
        data[NLMSG_HDR + IFINFOMSG + RTA_HDR] = b'x';
        nl.send(&data).unwrap();
        let err = replies(&nl);
        assert_eq!(
            i32::from_le_bytes(err[0][16..20].try_into().unwrap()),
            -Errno::ENODEV.0
        );
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
            nl.send(&data[..16 * 1024]).unwrap();
            sent += 1;
        }
        let q = nl.queue();
        assert!(q.queued <= q.rcvbuf + 4096, "bounded by SO_RCVBUF");
        assert_eq!(q.err, Some(Errno::ENOBUFS));
        drop(q);
        assert!(nl.poll(None).contains(PollMask::ERR));
        assert_eq!(nl.send(&vec![0; BUF_DEFAULT]), Err(Errno::EMSGSIZE));
    }
}
