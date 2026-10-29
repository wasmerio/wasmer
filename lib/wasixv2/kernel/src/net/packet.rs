//! `AF_PACKET` sockets (`net/packet/af_packet.c`), as far as a network without frames has them:
//! made, bound to an interface and named as Linux does, and answering the interface requests
//! every socket answers, which is what programs such as LTP's `tst_ns_ifmove` open one for.
//! No frame ever exists: nothing is received, and a send that passes Linux's checks fails with
//! `EOPNOTSUPP` instead of putting a frame on the link.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{AddressFamily, MsgFlags, OpenFlags, SocketFlags, SocketOption, SocketType};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, Fd, LocalFuture, SysResult};
use wasixv2_vfs::Cap;
use wasixv2_vnet::{
    bpf::SockFilter,
    iface::flags::{IFF_LOOPBACK, IFF_UP},
};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait};

use crate::{
    Kernel,
    file::{Backing, FileObject, Io, OpenFile},
    ns::NetNs,
    socket::{Blocking, RCV, Timeouts, WaitOn},
    sync::MutexExt,
};

/// `sizeof(struct sockaddr_ll)` and `offsetof(struct sockaddr_ll, sll_addr)`.
const SOCKADDR_LL: usize = 20;
const SLL_ADDR: usize = 12;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;
/// `SO_SNDBUF` and `SO_RCVBUF`'s default.
const BUF_DEFAULT: i32 = 212_992;
/// `PACKET_VERSION`, and `TPACKET_V1`, the format without a ring of its own.
const PACKET_VERSION: i32 = 10;
const TPACKET_V1: i32 = 0;

pub(crate) struct PacketSocket {
    kind: SocketType,
    /// The network namespace the socket was made in (`sock_net`).
    pub(crate) net: Arc<NetNs>,
    /// `po->num` (network byte order, as given) and `po->ifindex`.
    bound: Mutex<(u16, u32)>,
    /// Woken by nothing: no frame arrives.
    wait: WaitQueue,
    pub(crate) timeouts: Timeouts,
    /// `SO_ATTACH_FILTER`'s program, which no frame ever runs through.
    filter: Mutex<SockFilter>,
    /// `po->tp_version`.
    tp_version: Mutex<i32>,
    host: Arc<dyn Host>,
}

impl PacketSocket {
    /// `packet_bind`: a `sockaddr_ll` naming an interface of the namespace (0 for every one),
    /// and the protocol to take (0 keeps the socket's).
    pub(crate) fn bind(&self, raw: &[u8]) -> Result<i32, Errno> {
        if raw.len() < SOCKADDR_LL
            || i32::from(u16::from_le_bytes([raw[0], raw[1]])) != AddressFamily::AF_PACKET.0
        {
            return Err(Errno::EINVAL);
        }
        let protocol = u16::from_le_bytes([raw[2], raw[3]]);
        let index = i32::from_le_bytes(raw[4..8].try_into().unwrap());
        if index != 0 && !self.interfaces().iter().any(|i| i.index as i32 == index) {
            return Err(Errno::ENODEV);
        }
        let mut bound = self.bound.lock_or_recover();
        if protocol != 0 {
            bound.0 = protocol;
        }
        bound.1 = index as u32;
        Ok(0)
    }

    fn interfaces(&self) -> Vec<wasixv2_vnet::Interface> {
        self.net
            .network
            .as_ref()
            .map(|n| n.interfaces())
            .unwrap_or_default()
    }

    /// `packet_getname`: the protocol, the bound interface with its hardware type and address.
    pub(crate) fn local_addr(&self) -> Vec<u8> {
        let (protocol, index) = *self.bound.lock_or_recover();
        let mut raw = vec![0; SOCKADDR_LL];
        raw[..2].copy_from_slice(&(AddressFamily::AF_PACKET.0 as u16).to_le_bytes());
        raw[2..4].copy_from_slice(&protocol.to_le_bytes());
        raw[4..8].copy_from_slice(&(index as i32).to_le_bytes());
        let iface = self.interfaces().into_iter().find(|i| i.index == index);
        let Some(iface) = iface else {
            raw.truncate(SLL_ADDR);
            return raw;
        };
        let hatype = match iface.flags & IFF_LOOPBACK != 0 {
            true => ARPHRD_LOOPBACK,
            false => ARPHRD_ETHER,
        };
        raw[8..10].copy_from_slice(&hatype.to_le_bytes());
        raw[11] = 6;
        raw[SLL_ADDR..SLL_ADDR + 6].copy_from_slice(&iface.mac);
        raw.truncate(SLL_ADDR + 6);
        raw
    }

    /// `packet_sendmsg` up to where a frame would go out: the interface (the bound one, or the
    /// destination's) must exist (`ENXIO`) and be up (`ENETDOWN`).
    pub(crate) fn send(&self, to: Option<&[u8]>) -> Result<usize, Errno> {
        let index = match to {
            None => self.bound.lock_or_recover().1,
            Some(raw) if raw.len() < SLL_ADDR => return Err(Errno::EINVAL),
            Some(raw) => u32::from_le_bytes(raw[4..8].try_into().unwrap()),
        };
        let iface = self
            .interfaces()
            .into_iter()
            .find(|i| i.index == index)
            .ok_or(Errno::ENXIO)?;
        if iface.flags & IFF_UP == 0 {
            return Err(Errno::ENETDOWN);
        }
        Err(Errno::EOPNOTSUPP)
    }

    /// A receive: no frame ever arrives, so it waits until interrupted or timed out, or fails
    /// with `EAGAIN` at once when non-blocking.
    pub(crate) async fn recv(&self, wait: &mut Blocking<'_>) -> SysResult<Vec<u8>> {
        wait.wait(WaitOn::Queue(&self.wait), PollMask::READABLE, || {
            Err::<Vec<u8>, _>(Errno::EAGAIN)
        })
        .await
    }

    pub(crate) fn blocking<'a>(&'a self, io: Io<'a>, dir: usize, flags: MsgFlags) -> Blocking<'a> {
        Blocking::new(&*self.host, io, flags, self.timeouts.get(dir))
    }

    /// `sk_setsockopt` for this socket: the filter's options; the rest only describe the
    /// descriptor or frames, which never come.
    pub(crate) fn setsockopt(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
        match SockFilter::handles(name) {
            true => self.filter.lock_or_recover().set(name, value),
            false => Ok(()),
        }
    }

    /// `packet_setsockopt` at `SOL_PACKET`. `PACKET_VERSION` takes the ring formats Linux
    /// knows, but a ring is mapped memory frames are written into, and there are no frames: a
    /// format with a ring of its own (`TPACKET_V2`, `TPACKET_V3`) is refused as a kernel
    /// without it refuses it (`EINVAL`), which tells programs to read with `recvfrom`. The other
    /// options (memberships, rings, fanout) are about frames too (`ENOPROTOOPT`).
    pub(crate) fn set_packet_option(&self, name: i32, value: &[u8]) -> Result<(), Errno> {
        if name != PACKET_VERSION {
            return Err(Errno::ENOPROTOOPT);
        }
        let value: [u8; 4] = value.try_into().map_err(|_| Errno::EINVAL)?;
        match i32::from_le_bytes(value) {
            TPACKET_V1 => {
                *self.tp_version.lock_or_recover() = TPACKET_V1;
                Ok(())
            }
            // `TPACKET_V2` and `TPACKET_V3` exist, but not here; anything else never did.
            _ => Err(Errno::EINVAL),
        }
    }

    /// `packet_getsockopt` at `SOL_PACKET`: `PACKET_VERSION`.
    pub(crate) fn packet_option(&self, name: i32) -> Result<Vec<u8>, Errno> {
        match name {
            PACKET_VERSION => Ok(self.tp_version.lock_or_recover().to_le_bytes().to_vec()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// `SOL_SOCKET` options as `sock_getsockopt` answers them for this socket.
    pub(crate) fn getsockopt(&self, name: i32) -> Result<Vec<u8>, Errno> {
        let int = |v: i32| Ok(v.to_le_bytes().to_vec());
        if SockFilter::handles(name) {
            let mut out = vec![0; wasixv2_vnet::bpf::MAX_PROGRAM_LEN];
            let n = self.filter.lock_or_recover().get(name, &mut out)?;
            out.truncate(n);
            return Ok(out);
        }
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(self.kind.0),
            SocketOption::SO_PROTOCOL => int(i32::from(self.bound.lock_or_recover().0)),
            SocketOption::SO_DOMAIN => int(AddressFamily::AF_PACKET.0),
            SocketOption::SO_ERROR => int(0),
            SocketOption::SO_SNDBUF | SocketOption::SO_RCVBUF => int(BUF_DEFAULT),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }
}

pub(crate) fn downcast(object: Arc<dyn FileObject>) -> Option<Arc<PacketSocket>> {
    let any: Arc<dyn std::any::Any + Send + Sync> = object;
    any.downcast().ok()
}

impl Pollable for PacketSocket {
    /// `datagram_poll`: writable, never readable.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.wait);
        PollMask::WRITABLE
    }
}

impl FileObject for PacketSocket {
    fn messages(&self) -> bool {
        true
    }

    fn read<'a>(&'a self, io: Io<'a>, _buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.blocking(io, RCV, MsgFlags(0));
            self.recv(&mut wait).await.map(|frame| frame.len())
        })
    }

    fn write<'a>(&'a self, _io: Io<'a>, _data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.send(None)?) })
    }
}

impl Kernel {
    /// `packet_create`: `CAP_NET_RAW` over the owner of the caller's network namespace
    /// (`ns_capable(net->user_ns, CAP_NET_RAW)`), then a raw or datagram socket; `protocol` is
    /// the Ethernet type in network byte order, as given.
    pub(crate) fn packet_socket(
        &self,
        caller: &Caller<'_>,
        kind: SocketType,
        protocol: i32,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        if !self.net_capable(caller, Cap::NetRaw) {
            return Err(Errno::EPERM.into());
        }
        if kind != SocketType::SOCK_RAW && kind != SocketType::SOCK_DGRAM {
            return Err(Errno::ESOCKTNOSUPPORT.into());
        }
        let protocol = u16::try_from(protocol).map_err(|_| Errno::EINVAL)?;
        let socket = PacketSocket {
            kind,
            net: self.ns(caller).net.clone(),
            bound: Mutex::new((protocol, 0)),
            wait: WaitQueue::new(),
            timeouts: Timeouts::default(),
            filter: Mutex::new(SockFilter::default()),
            tp_version: Mutex::new(TPACKET_V1),
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
