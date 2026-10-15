//! `AF_NETLINK` sockets with the `NETLINK_ROUTE` basics: `RTM_GETLINK` and `RTM_GETADDR` dumps
//! of the network namespace's interface table, which is what `getifaddrs` and
//! `if_nameindex` ask for. Replies are queued when the request is sent, so a non-blocking
//! `recv` right after the `send` finds them, as on Linux.
//!
//! Not yet: routes (`RTM_GETROUTE`), multicast groups, changes (`RTM_NEWADDR` from the guest),
//! and other netlink families.

use std::{
    collections::VecDeque,
    net::IpAddr,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{AddressFamily, MsgFlags, OpenFlags, SocketFlags, SocketOption, SocketType};
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysErr, SysResult};
use wasixv2_vnet::Interface;
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
};

const NETLINK_ROUTE: i32 = 0;
const NLMSG_HDR: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_MULTI: u16 = 2;
const NLM_F_DUMP: u16 = 0x300;
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
/// `sizeof(struct sockaddr_nl)`.
const SOCKADDR_NL: usize = 12;

pub(crate) struct NetlinkSocket {
    replies: Mutex<VecDeque<Vec<u8>>>,
    wait: WaitQueue,
    interfaces: Vec<Interface>,
    port: u32,
}

impl NetlinkSocket {
    /// Handles one request datagram and queues the replies.
    pub(crate) fn send(&self, data: &[u8]) -> Result<usize, Errno> {
        let mut at = 0;
        while data.len() - at >= NLMSG_HDR {
            let h = &data[at..];
            let len = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
            if len < NLMSG_HDR || len > h.len() {
                break;
            }
            let kind = u16::from_le_bytes([h[4], h[5]]);
            let flags = u16::from_le_bytes([h[6], h[7]]);
            let seq = u32::from_le_bytes(h[8..12].try_into().unwrap());
            let family = h.get(NLMSG_HDR).copied().unwrap_or(0);
            let reply = match kind {
                RTM_GETLINK if flags & NLM_F_DUMP != 0 => self.dump(seq, |out| {
                    for iface in &self.interfaces {
                        out.push(link_msg(iface, seq, self.port));
                    }
                }),
                RTM_GETADDR if flags & NLM_F_DUMP != 0 => self.dump(seq, |out| {
                    for iface in &self.interfaces {
                        for &(ip, prefix) in &iface.addresses {
                            let wanted = match family as i32 {
                                0 => true,
                                f if f == AddressFamily::AF_INET.0 => ip.is_ipv4(),
                                f if f == AddressFamily::AF_INET6.0 => ip.is_ipv6(),
                                _ => false,
                            };
                            if wanted {
                                out.push(addr_msg(iface, ip, prefix, seq, self.port));
                            }
                        }
                    }
                }),
                _ => error_msg(h, len, seq, self.port, Errno::EOPNOTSUPP),
            };
            self.replies.lock().unwrap().push_back(reply);
            at += len.next_multiple_of(4);
        }
        self.wait.wake(PollMask::READABLE);
        Ok(data.len())
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

    /// Receives one queued datagram; returns the bytes copied and its full length.
    pub(crate) async fn recv(
        &self,
        io: Io<'_>,
        buf: &mut [u8],
        flags: MsgFlags,
    ) -> SysResult<(usize, usize)> {
        let peek = flags.contains(MsgFlags::MSG_PEEK);
        let take = || {
            let mut replies = self.replies.lock().unwrap();
            let reply = match peek {
                true => replies.front().cloned(),
                false => replies.pop_front(),
            };
            match reply {
                Some(reply) => Some(Ok(reply)),
                None => io.nonblocking().then_some(Err(Errno::EAGAIN)),
            }
        };
        let reply = wait_event(&self.wait, take, io.interrupt)
            .await
            .map_err(|_| SysErr::Restart)??;
        let n = reply.len().min(buf.len());
        buf[..n].copy_from_slice(&reply[..n]);
        Ok((n, reply.len()))
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

    pub(crate) fn getsockopt(&self, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(SocketType::SOCK_RAW.0),
            SocketOption::SO_PROTOCOL => int(NETLINK_ROUTE),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_NETLINK.0),
            SocketOption::SO_ERROR => int(0),
            SocketOption::SO_SNDBUF | SocketOption::SO_RCVBUF => int(212_992),
            _ => Err(Errno::ENOPROTOOPT),
        }
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

fn nlmsg(kind: u16, flags: u16, seq: u32, port: u32, payload: &[u8]) -> Vec<u8> {
    let len = NLMSG_HDR + payload.len();
    let mut out = Vec::with_capacity(len.next_multiple_of(4));
    out.extend((len as u32).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(seq.to_le_bytes());
    out.extend(port.to_le_bytes());
    out.extend(payload);
    out.resize(len.next_multiple_of(4), 0);
    out
}

fn rta(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
    let len = 4 + data.len();
    out.extend((len as u16).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// `RTM_NEWLINK`: `struct ifinfomsg` plus name, MTU and hardware addresses.
fn link_msg(iface: &Interface, seq: u32, port: u32) -> Vec<u8> {
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
    nlmsg(RTM_NEWLINK, NLM_F_MULTI, seq, port, &p)
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

/// `NLMSG_ERROR` echoing the request's header.
fn error_msg(request: &[u8], len: usize, seq: u32, port: u32, errno: Errno) -> Vec<u8> {
    let mut p = (-errno.0).to_le_bytes().to_vec();
    p.extend(&request[..NLMSG_HDR.min(len)]);
    nlmsg(NLMSG_ERROR, 0, seq, port, &p)
}

impl Pollable for NetlinkSocket {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wait);
        let mut mask = PollMask::WRITABLE;
        if !self.replies.lock().unwrap().is_empty() {
            mask |= PollMask::READABLE;
        }
        mask
    }
}

impl FileObject for NetlinkSocket {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.recv(io, buf, MsgFlags(0)).await?.0) })
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
        let interfaces = self
            .network
            .as_ref()
            .map(|net| net.interfaces())
            .unwrap_or_default();
        let socket = NetlinkSocket {
            replies: Mutex::default(),
            wait: WaitQueue::new(),
            interfaces,
            port: self.process(caller.task()).pid.0,
        };
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(Backing::Stream(Arc::new(socket)), status);
        let [fd] = self.install_files(caller, [file], flags.contains(SocketFlags::SOCK_CLOEXEC))?;
        Ok(fd)
    }
}
