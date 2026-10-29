//! The socket syscalls, as Linux's `net/socket.c`: descriptor handling, guest struct decoding
//! (`move_addr_to_kernel`, `move_addr_to_user`, msghdr and cmsg layouts), moving data between
//! guest iovecs and sockets, and the `SOL_SOCKET` options the socket layer itself answers. IP
//! protocol behaviour comes from the network namespace (`wasixv2_vnet::Network`); Unix and
//! netlink sockets are kernel facilities of their own.
//!
//! Everything a guest hands in is checked as Linux checks it: lengths are validated before
//! they are used and address arithmetic that would leave the address space is `EFAULT`, so no
//! guest input can make the host panic.

use std::{cell::RefCell, net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::{
    AddressFamily, Buf, ConstSockaddrPtr, ConstUserMsghdrPtr, FdPair, GuestLayout, GuestPtr,
    GuestPtrMut, Iovec, KernelOldTimeval, KernelSockTimeval, MmsghdrArray, MsgFlags, OpenFlags,
    ShutdownHow, SockaddrPtr, SocketFlags, SocketLevel, SocketOption, SocketType, Socklen,
    SocklenPtr, UserMsghdr, UserMsghdrPtr,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, LocalFuture, SysErr, SysResult, UserMemory, UserSlice, UserSliceMut,
    read_value, write_value,
};
use wasixv2_vfs::{Cap, Path};
use wasixv2_vnet::{Control, Domain, SendMsg, Shutdown, SocketKind};
use wasixv2_wait::PollMask;

mod if_ioctl;
mod packet;

pub(crate) use if_ioctl::is_dev_ioctl;

use crate::{
    Kernel,
    cred::Cred,
    file::{Backing, OpenFile, ReadSink},
    fs::read_iovecs,
    netlink,
    ns::NetNs,
    socket::{self, RCV, SND, SOCKADDR_STORAGE, SocketFile, Timeouts, WaitOn},
    sync::MutexExt,
    unix::{self, UnixFile},
};

/// `/proc/sys/net/core/somaxconn`.
const SOMAXCONN: u32 = 4096;
/// `UIO_MAXIOV`, also the most messages one `sendmmsg` or `recvmmsg` handles.
const UIO_MAXIOV: u32 = 1024;
/// `SOCK_TYPE_MASK`: the type bits of `socket`'s type argument.
const SOCK_TYPE_MASK: i32 = 0xf;
/// `SOCK_MAX`.
const SOCK_MAX: i32 = 11;
/// `sizeof(struct cmsghdr)` and its alignment on wasm32 (`CMSG_ALIGN`, `sizeof(long)`).
const CMSG_HDR: usize = 12;
const CMSG_ALIGN: usize = 4;
/// `net.core.optmem_max`: the most control data one `sendmsg` may hand in.
const OPTMEM_MAX: usize = 128 * 1024;
/// `MAX_RW_COUNT`: the most one call transfers.
const MAX_RW_COUNT: u32 = 0x7fff_f000;
/// What a stream send or receive moves through kernel memory per step.
const SOCK_CHUNK: usize = 256 * 1024;
/// The largest IP datagram payload. A send copies one byte more, so the protocol sees an
/// oversized datagram and reports `EMSGSIZE` in its own order.
const IP_MAX_DATAGRAM: usize = 65535;

/// An open socket: its open file and its socket state.
type Sock = (Arc<OpenFile>, Arc<SocketFile>);

/// An open socket of any family the kernel implements.
enum AnySock {
    Inet(Sock),
    Unix(Arc<OpenFile>, Arc<unix::Sock>),
    Netlink(Arc<OpenFile>, Arc<netlink::NetlinkSocket>),
    Packet(Arc<OpenFile>, Arc<packet::PacketSocket>),
}

impl AnySock {
    fn file(&self) -> &OpenFile {
        match self {
            AnySock::Inet((file, _))
            | AnySock::Unix(file, _)
            | AnySock::Netlink(file, _)
            | AnySock::Packet(file, _) => file,
        }
    }

    fn timeouts(&self) -> &Timeouts {
        match self {
            AnySock::Inet((_, sock)) => &sock.timeouts,
            AnySock::Unix(_, sock) => &sock.timeouts,
            AnySock::Netlink(_, nl) => &nl.timeouts,
            AnySock::Packet(_, p) => &p.timeouts,
        }
    }

    /// Stream sockets raise `SIGPIPE` for `EPIPE` (`sk_stream_error`, `unix_stream_sendmsg`);
    /// datagram ones never do.
    fn raises_sigpipe(&self) -> bool {
        match self {
            AnySock::Inet((_, sock)) => sock.socket.kind() == SocketKind::Stream,
            AnySock::Unix(_, sock) => sock.kind() == unix::Kind::Stream,
            AnySock::Netlink(..) | AnySock::Packet(..) => false,
        }
    }
}

/// What one receive produced, for `recvfrom` and `recv_msghdr`.
struct Got {
    len: usize,
    result: usize,
    from: Option<Vec<u8>>,
    flags: MsgFlags,
    controls: Vec<Control>,
    rights: Vec<Arc<OpenFile>>,
}

impl Kernel {
    /// `sockfd_lookup` for every family.
    fn any_socket(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<AnySock> {
        let file = self.file(caller.task(), fd)?;
        if let Some(unix) = file.downcast::<UnixFile>() {
            let sock = unix.0.clone();
            return Ok(AnySock::Unix(file, sock));
        }
        if let Some(nl) = file.object().cloned().and_then(netlink::downcast) {
            return Ok(AnySock::Netlink(file, nl));
        }
        if let Some(p) = file.object().cloned().and_then(packet::downcast) {
            return Ok(AnySock::Packet(file, p));
        }
        self.socket_fd(caller, fd).map(AnySock::Inet)
    }

    fn install_unix(
        &self,
        caller: &Caller<'_>,
        sock: Arc<unix::Sock>,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        let owner = self.cred(caller).fs_owner();
        let file = unix_file(sock, flags, self.pseudo.socket(owner));
        let [fd] = self.install_files(caller, [file], flags.contains(SocketFlags::SOCK_CLOEXEC))?;
        Ok(fd)
    }

    /// `sockfd_lookup`: `EBADF`, then `ENOTSOCK` for a descriptor that is not a socket.
    fn socket_fd(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Sock> {
        let file = self.file(caller.task(), fd)?;
        let object = file.object().ok_or(Errno::ENOTSOCK)?.clone();
        let any: Arc<dyn std::any::Any + Send + Sync> = object;
        let socket = any.downcast::<SocketFile>().map_err(|_| Errno::ENOTSOCK)?;
        Ok((file, socket))
    }

    /// An open file for the IP socket `socket` of the network namespace `net`.
    fn socket_file(
        &self,
        caller: &Caller<'_>,
        socket: Arc<dyn wasixv2_vnet::Socket>,
        net: Arc<NetNs>,
        flags: SocketFlags,
    ) -> OpenFile {
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let path = self.pseudo.socket(self.cred(caller).fs_owner());
        socket.graft(path.dentry.node().ino());
        let object = SocketFile::new(socket, net, self.host.clone());
        OpenFile::new(Backing::Stream(Arc::new(object), path), status)
    }

    /// `__sys_socket` and `__sock_create`.
    pub(crate) async fn sys_socket(
        &self,
        caller: &Caller<'_>,
        family: AddressFamily,
        r#type: i32,
        protocol: i32,
    ) -> SysResult<Fd> {
        let (kind, flags) = socket_type(r#type)?;
        let domain = match family {
            AddressFamily::AF_INET => Domain::Inet,
            AddressFamily::AF_INET6 => Domain::Inet6,
            AddressFamily::AF_UNIX => {
                let sock = self.unix_socket(caller, kind, protocol)?;
                return self.install_unix(caller, sock, flags);
            }
            AddressFamily::AF_NETLINK => return self.netlink_socket(caller, kind, protocol, flags),
            AddressFamily::AF_PACKET => return self.packet_socket(caller, kind, protocol, flags),
            // Every other family (and anything past NPROTO) has no implementation.
            _ => return Err(Errno::EAFNOSUPPORT.into()),
        };
        let kind = match kind {
            SocketType::SOCK_STREAM => SocketKind::Stream,
            SocketType::SOCK_DGRAM => SocketKind::Datagram,
            // `inet_create`: a raw socket names its protocol, and needs CAP_NET_RAW.
            SocketType::SOCK_RAW if protocol == 0 => return Err(Errno::EPROTONOSUPPORT.into()),
            SocketType::SOCK_RAW if !self.cred(caller).capable(Cap::NetRaw) => {
                return Err(Errno::EPERM.into());
            }
            SocketType::SOCK_RAW => SocketKind::Raw,
            _ => return Err(Errno::ESOCKTNOSUPPORT.into()),
        };
        let ns = self.ns(caller);
        let network = ns.net.network.as_ref().ok_or(Errno::EAFNOSUPPORT)?;
        let icmp = match domain {
            Domain::Inet => wasixv2_abi::IpProtocol::IPPROTO_ICMP.0,
            Domain::Inet6 => wasixv2_abi::IpProtocol::IPPROTO_ICMPV6.0,
        };
        if kind == SocketKind::Datagram && protocol == icmp {
            self.ping_allowed(caller, &ns.net)?;
        }
        // `sk_uid`: the inode owner `sock_alloc` gives the socket.
        let (uid, _) = self.cred(caller).fs_owner();
        let socket = network.socket(domain, kind, protocol, uid)?;
        let file = self.socket_file(caller, socket, ns.net.clone(), flags);
        let cloexec = flags.contains(SocketFlags::SOCK_CLOEXEC);
        let [fd] = self.install_files(caller, [file], cloexec)?;
        Ok(fd)
    }

    /// `ping_init_sock`: an ICMP datagram socket is for the groups `ping_group_range` names,
    /// the effective group or a supplementary one; `EACCES` otherwise, root included.
    fn ping_allowed(&self, caller: &Caller<'_>, net: &NetNs) -> Result<(), Errno> {
        let (low, high) = net.sysctl.lock_or_recover().ping_group_range();
        let cred = self.cred(caller);
        let allowed = |gid: &u32| (low..=high).contains(gid);
        match allowed(&cred.group.effective) || cred.groups.iter().any(allowed) {
            true => Ok(()),
            false => Err(Errno::EACCES),
        }
    }

    /// `__sys_socketpair`. Only Unix sockets have pairs.
    pub(crate) async fn sys_socketpair(
        &self,
        caller: &Caller<'_>,
        family: AddressFamily,
        r#type: i32,
        protocol: i32,
        usockvec: FdPair,
    ) -> SysResult<i32> {
        let (kind, flags) = socket_type(r#type)?;
        match family {
            AddressFamily::AF_UNIX => {}
            AddressFamily::AF_INET | AddressFamily::AF_INET6 => {
                return Err(Errno::EOPNOTSUPP.into());
            }
            _ => return Err(Errno::EAFNOSUPPORT.into()),
        }
        let a = self.unix_socket(caller, kind, protocol)?;
        let b = self.unix_socket(caller, kind, protocol)?;
        unix::Sock::pair(&a, &b);
        let owner = self.cred(caller).fs_owner();
        let files = [
            unix_file(a, flags, self.pseudo.socket(owner)),
            unix_file(b, flags, self.pseudo.socket(owner)),
        ];
        let cloexec = flags.contains(SocketFlags::SOCK_CLOEXEC);
        self.install_pair(caller, usockvec, files, cloexec).await?;
        Ok(0)
    }

    /// `__sys_bind` and `inet_bind`.
    pub(crate) async fn sys_bind(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: ConstSockaddrPtr,
        len: Socklen,
    ) -> SysResult<i32> {
        let raw = move_addr_to_kernel(caller, addr, len).await?;
        let sock = match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => sock,
            AnySock::Unix(_, sock) => return self.unix_bind(caller, &sock, &raw).await,
            AnySock::Netlink(_, nl) => return Ok(nl.bind(&raw)?),
            AnySock::Packet(_, p) => return Ok(p.bind(&raw)?),
        };
        let domain = sock.socket.domain();
        let addr = socket::decode(domain, &raw, true)?;
        // AF_UNSPEC is accepted for IPv4 only with INADDR_ANY.
        if socket::family(&raw)? == AddressFamily::AF_UNSPEC && !addr.ip().is_unspecified() {
            return Err(Errno::EAFNOSUPPORT.into());
        }
        let privileged = self.cred(caller).capable(Cap::NetBindService);
        sock.socket.bind(addr, privileged)?;
        Ok(0)
    }

    /// `__sys_listen`.
    pub(crate) async fn sys_listen(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        backlog: i32,
    ) -> SysResult<i32> {
        let backlog = (backlog as u32).min(SOMAXCONN);
        match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => sock.socket.listen(backlog)?,
            AnySock::Unix(_, sock) => sock.listen(backlog, self.peer_cred(caller))?,
            AnySock::Netlink(..) | AnySock::Packet(..) => return Err(Errno::EOPNOTSUPP.into()),
        }
        Ok(0)
    }

    /// `do_accept`: waits for a connection unless non-blocking. As Linux, the descriptor is
    /// reserved before the connection is taken (`get_unused_fd_flags`), so a full table fails
    /// with `EMFILE` and leaves the connection queued, and installed only once the peer's
    /// address is written (`fd_install` last): a fault there closes the accepted socket, which
    /// its peer sees, and no other thread ever saw its descriptor.
    pub(crate) async fn sys_accept4(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: SockaddrPtr,
        addrlen: SocklenPtr,
        flags: SocketFlags,
    ) -> SysResult<Fd> {
        if !(flags & !(SocketFlags::SOCK_CLOEXEC | SocketFlags::SOCK_NONBLOCK)).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let any = self.any_socket(caller, fd)?;
        let cloexec = flags.contains(SocketFlags::SOCK_CLOEXEC);
        let [slot] = self.reserve_fds::<1>(caller, cloexec)?;
        let interrupt = self.interrupt(caller);
        let io = Self::io(caller, &*interrupt, any.file());
        let (file, peer) = match &any {
            AnySock::Inet((_, sock)) => {
                let mut wait = sock.blocking(io, RCV, MsgFlags(0));
                let on = WaitOn::Object(&*sock.socket);
                wait.wait(on, PollMask::READABLE, || {
                    let accepted = sock.socket.accept()?;
                    let peer = socket::encode(accepted.peer);
                    let net = sock.net.clone();
                    Ok((self.socket_file(caller, accepted.socket, net, flags), peer))
                })
                .await?
            }
            AnySock::Unix(_, sock) => {
                let mut wait = sock.blocking(io, RCV, MsgFlags(0));
                wait.wait(WaitOn::Queue(sock.queue()), PollMask::READABLE, || {
                    if sock.kind() == unix::Kind::Dgram {
                        return Err(Errno::EOPNOTSUPP);
                    }
                    // The embryo is an open file at once, so every way out releases it.
                    let embryo = sock.try_accept()?;
                    let peer = embryo.peer_addr().unwrap_or_default();
                    let owner = self.cred(caller).fs_owner();
                    Ok((unix_file(embryo, flags, self.pseudo.socket(owner)), peer))
                })
                .await?
            }
            AnySock::Netlink(..) | AnySock::Packet(..) => return Err(Errno::EOPNOTSUPP.into()),
        };
        if !addr.is_null() {
            move_addr_to_user(caller, &peer, addr, addrlen).await?;
        }
        Ok(slot.install(Arc::new(file)))
    }

    /// `__sys_connect`, `inet_stream_connect` and `inet_dgram_connect`.
    pub(crate) async fn sys_connect(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: ConstSockaddrPtr,
        len: Socklen,
    ) -> SysResult<i32> {
        let raw = move_addr_to_kernel(caller, addr, len).await?;
        let (file, sock) = match self.any_socket(caller, fd)? {
            AnySock::Inet(sock) => sock,
            AnySock::Unix(file, sock) => {
                return self.unix_connect(caller, &file, &sock, &raw).await;
            }
            AnySock::Netlink(_, nl) => {
                let net_admin = self.cred(caller).capable(Cap::NetAdmin);
                nl.connect(&raw, net_admin)?;
                return Ok(0);
            }
            // `sock_no_connect`.
            AnySock::Packet(..) => return Err(Errno::EOPNOTSUPP.into()),
        };
        if socket::family(&raw)? == AddressFamily::AF_UNSPEC {
            sock.socket.disconnect()?;
            return Ok(0);
        }
        let socket = &sock.socket;
        let addr = match socket.kind() {
            // `ip6_datagram_connect` takes an IPv4 address too.
            SocketKind::Datagram | SocketKind::Raw => {
                socket::decode_dest(socket.domain(), &raw)?.ok_or(Errno::EAFNOSUPPORT)?
            }
            SocketKind::Stream => socket::decode(socket.domain(), &raw, false)?,
        };
        let interrupt = self.interrupt(caller);
        let mut wait = sock.blocking(Self::io(caller, &*interrupt, &file), SND, MsgFlags(0));
        // `__inet_stream_connect`: a blocking socket waits for a connection under way, its
        // own or one a restarted call left (`EALREADY`); a non-blocking one reports it.
        match socket.connect(addr) {
            Err(Errno::EINPROGRESS | Errno::EALREADY) if !wait.nonblocking() => {}
            result => return Ok(result.map(|()| 0)?),
        }
        let finished = || match socket.connect(addr) {
            Err(Errno::EALREADY | Errno::EINPROGRESS) => Err(Errno::EAGAIN),
            Err(Errno::EISCONN) => Ok(()),
            result => result,
        };
        match wait
            .wait(WaitOn::Object(&**socket), PollMask::WRITABLE, finished)
            .await
        {
            // `inet_wait_for_connect` timed out.
            Err(SysErr::Errno(Errno::EAGAIN)) => Err(Errno::EINPROGRESS.into()),
            result => result.map(|()| 0),
        }
    }

    /// `__sys_getsockname`.
    pub(crate) async fn sys_getsockname(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: SockaddrPtr,
        addrlen: SocklenPtr,
    ) -> SysResult<i32> {
        let raw = match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => socket::encode(sock.socket.local_addr()?),
            AnySock::Unix(_, sock) => sock.local_addr(),
            AnySock::Netlink(_, nl) => nl.local_addr(),
            AnySock::Packet(_, p) => p.local_addr(),
        };
        move_addr_to_user(caller, &raw, addr, addrlen).await?;
        Ok(0)
    }

    /// `__sys_getpeername`.
    pub(crate) async fn sys_getpeername(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: SockaddrPtr,
        addrlen: SocklenPtr,
    ) -> SysResult<i32> {
        let raw = match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => socket::encode(sock.socket.peer_addr()?),
            AnySock::Unix(_, sock) => sock.peer_addr()?,
            AnySock::Netlink(_, nl) => nl.peer_addr(),
            AnySock::Packet(..) => return Err(Errno::EOPNOTSUPP.into()),
        };
        move_addr_to_user(caller, &raw, addr, addrlen).await?;
        Ok(0)
    }

    /// `__sys_sendto`.
    pub(crate) async fn sys_sendto(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSlice,
        flags: MsgFlags,
        addr: ConstSockaddrPtr,
        len: Socklen,
    ) -> SysResult<u32> {
        let sock = self.any_socket(caller, fd)?;
        let to = match addr.is_null() {
            true => None,
            false => Some(move_addr_to_kernel(caller, addr, len).await?),
        };
        let src = GuestIov::single(buf.addr, buf.len);
        let sent = self
            .send_from(caller, &sock, &src, to.as_deref(), &[], flags)
            .await?;
        Ok(sent as u32)
    }

    /// `sock_sendmsg` for every family: the data of `src`, to the address `to` if given.
    /// Streams take it all unless non-blocking or interrupted, in steps through kernel memory;
    /// a datagram is copied whole, up to one byte past the largest the protocol takes.
    async fn send_from(
        &self,
        caller: &Caller<'_>,
        sock: &AnySock,
        src: &GuestIov,
        to: Option<&[u8]>,
        control: &[Control],
        flags: MsgFlags,
    ) -> SysResult<usize> {
        let memory = caller.memory();
        let interrupt = self.interrupt(caller);
        let io = Self::io(caller, &*interrupt, sock.file());
        let result = match sock {
            AnySock::Inet((_, sock)) => {
                let to = match to {
                    Some(raw) => self.destination(sock, raw)?,
                    None => None,
                };
                let cred = self.cred(caller);
                let control = sock_cmsg_send(&cred, control)?;
                let net_raw = cred.capable(Cap::NetRaw);
                let mut wait = sock.blocking(io, SND, flags);
                let mut scratch = Scratch::default();
                let stream = sock.socket.kind() == SocketKind::Stream;
                let mut sent = 0;
                loop {
                    let n = match stream {
                        true => (src.len - sent).min(SOCK_CHUNK),
                        false => src.len.min(IP_MAX_DATAGRAM + 1),
                    };
                    let buf = scratch.get(n);
                    if let Err(err) = src.copy_in(memory, sent, buf).await {
                        break if sent > 0 { Ok(sent) } else { Err(err) };
                    }
                    let msg = SendMsg {
                        data: buf,
                        to,
                        flags,
                        control: if sent == 0 { &control } else { &[] },
                        net_raw,
                    };
                    match sock.send(&mut wait, msg).await {
                        Ok(k) => {
                            sent += k;
                            if !stream || k < n || sent >= src.len {
                                break Ok(sent);
                            }
                        }
                        Err(_) if sent > 0 => break Ok(sent),
                        Err(err) => break Err(err),
                    }
                }
            }
            AnySock::Unix(_, sock) => {
                let to = self.unix_destination(caller, sock, to).await?;
                let anc = self.unix_ancillary(caller, control)?;
                let cred = self.ucred(caller);
                let mut wait = sock.blocking(io, SND, flags);
                match sock.max_record() {
                    Some(max) => {
                        let mut data = vec![0; src.len.min(max.saturating_add(1))];
                        src.copy_in(memory, 0, &mut data).await?;
                        unix::send(sock, &mut wait, data, to, anc, cred).await
                    }
                    None => {
                        let mut anc = Some(anc);
                        let mut sent = 0;
                        loop {
                            let mut data = vec![0; (src.len - sent).min(SOCK_CHUNK)];
                            let n = data.len();
                            if let Err(err) = src.copy_in(memory, sent, &mut data).await {
                                break if sent > 0 { Ok(sent) } else { Err(err) };
                            }
                            let anc = anc.take().unwrap_or_default();
                            match unix::send(sock, &mut wait, data, None, anc, cred).await {
                                Ok(k) => {
                                    sent += k;
                                    if k < n || sent >= src.len {
                                        break Ok(sent);
                                    }
                                }
                                Err(_) if sent > 0 => break Ok(sent),
                                Err(err) => break Err(err),
                            }
                        }
                    }
                }
            }
            AnySock::Netlink(_, nl) => {
                let mut data = vec![0; src.len.min(nl.max_request().saturating_add(1))];
                src.copy_in(memory, 0, &mut data).await?;
                let net_admin = self.cred(caller).capable(Cap::NetAdmin);
                let sender = NetlinkSender {
                    kernel: self,
                    caller,
                };
                Ok(nl.send(&data, net_admin, &sender)?)
            }
            AnySock::Packet(_, p) => Ok(p.send(to)?),
        };
        match sock.raises_sigpipe() && !flags.contains(MsgFlags::MSG_NOSIGNAL) {
            true => self.sigpipe(caller, result),
            false => result,
        }
    }

    /// A send's destination. Stream sockets ignore it, as TCP does.
    fn destination(&self, sock: &SocketFile, raw: &[u8]) -> Result<Option<SocketAddr>, Errno> {
        if sock.socket.kind() == SocketKind::Stream {
            return Ok(None);
        }
        socket::decode_dest(sock.socket.domain(), raw)
    }

    /// `__sys_recvfrom`.
    pub(crate) async fn sys_recvfrom(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        buf: UserSliceMut,
        flags: MsgFlags,
        addr: SockaddrPtr,
        addrlen: SocklenPtr,
    ) -> SysResult<u32> {
        let sock = self.any_socket(caller, fd)?;
        let dst = GuestIov::single(buf.addr, buf.len);
        let got = self.recv_into(caller, &sock, &dst, flags).await?;
        if !addr.is_null() {
            let raw = got.from.unwrap_or_default();
            move_addr_to_user(caller, &raw, addr, addrlen).await?;
        }
        Ok(got.result as u32)
    }

    /// `sock_recvmsg` for every family into `dst`: one record for datagram families; for
    /// streams what is there, and with `MSG_WAITALL` until `dst` is full, the stream ends or
    /// fails, in steps through kernel memory.
    async fn recv_into(
        &self,
        caller: &Caller<'_>,
        sock: &AnySock,
        dst: &GuestIov,
        flags: MsgFlags,
    ) -> SysResult<Got> {
        let memory = caller.memory();
        let interrupt = self.interrupt(caller);
        let io = Self::io(caller, &*interrupt, sock.file());
        let waitall = flags.contains(MsgFlags::MSG_WAITALL) && !flags.contains(MsgFlags::MSG_PEEK);
        match sock {
            AnySock::Inet((_, sock)) => {
                let stream = sock.socket.kind() == SocketKind::Stream;
                let keeps = stream && !flags.intersects(TCP_OTHER_READS);
                let mut wait = sock.blocking(io, RCV, flags);
                let mut scratch = Scratch::default();
                let n = match stream {
                    true => dst.len.min(SOCK_CHUNK),
                    false => dst.len.min(IP_MAX_DATAGRAM),
                };
                let buf = scratch.get(n);
                let msg = match keeps {
                    true => {
                        let mut to = IovOut::new(memory, dst, 0);
                        sock.recv_stream_to(&mut wait, buf, flags, &mut to).await?
                    }
                    false => {
                        let msg = sock.recv(&mut wait, buf, flags).await?;
                        dst.copy_out(memory, 0, &buf[..msg.len]).await?;
                        msg
                    }
                };
                let mut got = Got {
                    len: msg.len,
                    result: recv_result(sock, flags, &msg),
                    from: msg.from.map(socket::encode),
                    flags: msg.flags,
                    controls: msg.control,
                    rights: Vec::new(),
                };
                while stream && waitall && got.len > 0 && got.len < dst.len {
                    let buf = scratch.get((dst.len - got.len).min(SOCK_CHUNK));
                    let more = match keeps {
                        true => {
                            let mut to = IovOut::new(memory, dst, got.len);
                            sock.recv_stream_to(&mut wait, buf, flags, &mut to).await
                        }
                        false => match sock.recv(&mut wait, buf, flags).await {
                            Ok(more) => match dst.copy_out(memory, got.len, &buf[..more.len]).await
                            {
                                Ok(()) => Ok(more),
                                Err(err) => Err(err),
                            },
                            Err(err) => Err(err),
                        },
                    };
                    match more {
                        Ok(more) if more.len > 0 => got.len += more.len,
                        _ => break,
                    }
                }
                if stream {
                    got.result = got.len;
                }
                Ok(got)
            }
            AnySock::Unix(_, sock) => {
                let stream = sock.kind() == unix::Kind::Stream;
                let keeps = stream && !flags.contains(MsgFlags::MSG_PEEK);
                let mut wait = sock.blocking(io, RCV, flags);
                let max = match stream {
                    true => dst.len.min(SOCK_CHUNK),
                    false => dst.len,
                };
                let msg = match keeps {
                    true => {
                        let mut to = IovOut::new(memory, dst, 0);
                        unix::recv_stream_to(sock, &mut wait, max, None, &mut to).await?
                    }
                    false => {
                        let msg = unix::recv(sock, &mut wait, max, flags, None).await?;
                        dst.copy_out(memory, 0, &msg.data).await?;
                        msg
                    }
                };
                let controls = msg
                    .cred
                    .map(|cred| Control {
                        level: SocketLevel::SOL_SOCKET.0,
                        kind: unix::SCM_CREDENTIALS,
                        data: cred.to_bytes(),
                    })
                    .into_iter()
                    .collect();
                let mut got = Got {
                    len: msg.len(),
                    result: unix_result(sock, flags, &msg),
                    // An unnamed sender has an empty address.
                    from: Some(msg.from.unwrap_or_default()),
                    flags: msg.flags,
                    controls,
                    rights: msg.rights,
                };
                // `unix_stream_read_generic` keeps waiting, but never past files and, with
                // `SO_PASSCRED`, never into another writer's data.
                let writer = msg.cred;
                while stream && waitall && got.len > 0 && got.len < dst.len && got.rights.is_empty()
                {
                    let max = (dst.len - got.len).min(SOCK_CHUNK);
                    let mut to = IovOut::new(memory, dst, got.len);
                    match unix::recv_stream_to(sock, &mut wait, max, writer, &mut to).await {
                        Ok(more) if more.len() > 0 => {
                            got.len += more.len();
                            got.rights = more.rights;
                        }
                        _ => break,
                    }
                }
                if stream {
                    got.result = got.len;
                }
                Ok(got)
            }
            AnySock::Netlink(_, nl) => {
                let mut wait = nl.blocking(io, RCV, flags);
                let reply = nl.recv(&mut wait, flags).await?;
                let len = reply.len().min(dst.len);
                dst.copy_out(memory, 0, &reply[..len]).await?;
                let mut out = MsgFlags(0);
                if reply.len() > len {
                    out |= MsgFlags::MSG_TRUNC;
                }
                Ok(Got {
                    len,
                    result: match flags.contains(MsgFlags::MSG_TRUNC) {
                        true => reply.len(),
                        false => len,
                    },
                    from: Some(netlink::kernel_addr()),
                    flags: out,
                    controls: Vec::new(),
                    rights: Vec::new(),
                })
            }
            AnySock::Packet(_, p) => {
                let mut wait = p.blocking(io, RCV, flags);
                let frame = p.recv(&mut wait).await?;
                Ok(Got {
                    len: frame.len(),
                    result: frame.len(),
                    from: None,
                    flags: MsgFlags(0),
                    controls: Vec::new(),
                    rights: Vec::new(),
                })
            }
        }
    }

    /// `___sys_sendmsg`.
    pub(crate) async fn sys_sendmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        msg: ConstUserMsghdrPtr,
        flags: MsgFlags,
    ) -> SysResult<i32> {
        let sock = self.any_socket(caller, fd)?;
        let hdr = read_value(caller.memory(), msg).await?;
        Ok(self.send_msghdr(caller, &sock, &hdr, flags).await? as i32)
    }

    async fn send_msghdr(
        &self,
        caller: &Caller<'_>,
        sock: &AnySock,
        hdr: &UserMsghdr,
        flags: MsgFlags,
    ) -> SysResult<usize> {
        let namelen = msg_namelen(hdr)?;
        if hdr.msg_iovlen > UIO_MAXIOV {
            return Err(Errno::EMSGSIZE.into());
        }
        let raw_to = match hdr.msg_name.is_null() || namelen == 0 {
            true => None,
            false => Some(
                move_addr_to_kernel(caller, GuestPtr::new(hdr.msg_name.addr()), namelen).await?,
            ),
        };
        let src = GuestIov::of(caller.memory(), hdr).await?;
        let control = read_cmsgs(caller, hdr).await?;
        self.send_from(caller, sock, &src, raw_to.as_deref(), &control, flags)
            .await
    }

    /// `___sys_recvmsg`.
    pub(crate) async fn sys_recvmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        msg: UserMsghdrPtr,
        flags: MsgFlags,
    ) -> SysResult<i32> {
        let sock = self.any_socket(caller, fd)?;
        Ok(self.recv_msghdr(caller, &sock, msg.addr(), flags).await? as i32)
    }

    /// Receives into the msghdr at `at` and writes back its name, control and flags.
    async fn recv_msghdr(
        &self,
        caller: &Caller<'_>,
        sock: &AnySock,
        at: u32,
        flags: MsgFlags,
    ) -> SysResult<usize> {
        let memory = caller.memory();
        let mut hdr: UserMsghdr = read_value(memory, GuestPtr::new(at)).await?;
        let namelen = msg_namelen(&hdr)?;
        if hdr.msg_iovlen > UIO_MAXIOV {
            return Err(Errno::EMSGSIZE.into());
        }
        let dst = GuestIov::of(memory, &hdr).await?;
        let got = self.recv_into(caller, sock, &dst, flags).await?;

        // The name: as much as fits, with the real length reported.
        hdr.msg_namelen = 0;
        if let (false, Some(raw)) = (hdr.msg_name.is_null(), got.from) {
            let n = raw.len().min(namelen as usize);
            memory.write(hdr.msg_name.addr(), &raw[..n]).await?;
            hdr.msg_namelen = raw.len() as i32;
        }
        let mut out_flags = got.flags;
        let mut used = write_cmsgs(caller, &hdr, &got.controls, &mut out_flags).await?;
        if !got.rights.is_empty() {
            let cloexec = flags.contains(MsgFlags::MSG_CMSG_CLOEXEC);
            used += self
                .write_rights(caller, &hdr, used, got.rights, cloexec, &mut out_flags)
                .await?;
        }
        hdr.msg_controllen = used as u32;
        hdr.msg_flags = out_flags.0;
        write_value(memory, GuestPtrMut::new(at), &hdr).await?;
        Ok(got.result)
    }

    /// `scm_detach_fds`: installs as many received files as the control buffer has room for
    /// and drops the rest with `MSG_CTRUNC`. Returns the bytes used.
    async fn write_rights(
        &self,
        caller: &Caller<'_>,
        hdr: &UserMsghdr,
        used: usize,
        rights: Vec<Arc<OpenFile>>,
        cloexec: bool,
        flags: &mut MsgFlags,
    ) -> SysResult<usize> {
        let space = match hdr.msg_control.is_null() {
            true => 0,
            false => (hdr.msg_controllen as usize).saturating_sub(used),
        };
        let fit = space.saturating_sub(CMSG_HDR) / 4;
        if fit == 0 {
            *flags |= MsgFlags::MSG_CTRUNC;
            return Ok(0);
        }
        let at = offset(hdr.msg_control.addr(), used)?;
        let total = rights.len();
        let wanted = fit.min(total);
        // `scm_detach_fds`: descriptors past the limit are dropped, and the message says so.
        let fds: Vec<Fd> = {
            let table = self.files(caller.task());
            let mut table = table.lock_or_recover();
            rights
                .into_iter()
                .take(wanted)
                .map_while(|file| table.install(file, cloexec))
                .collect()
        };
        let n = fds.len();
        if n < total {
            *flags |= MsgFlags::MSG_CTRUNC;
        }
        if n == 0 {
            return Ok(0);
        }
        let len = CMSG_HDR + 4 * n;
        let mut raw = Vec::with_capacity(len);
        raw.extend((len as u32).to_le_bytes());
        raw.extend(SocketLevel::SOL_SOCKET.0.to_le_bytes());
        raw.extend(unix::SCM_RIGHTS.to_le_bytes());
        for fd in fds {
            raw.extend(fd.0.to_le_bytes());
        }
        caller.memory().write(at, &raw).await?;
        Ok(len.next_multiple_of(CMSG_ALIGN).min(space))
    }

    /// `__sys_sendmmsg`: stops at the first failure, which is reported only if nothing was
    /// sent.
    pub(crate) async fn sys_sendmmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mmsg: MmsghdrArray,
        vlen: u32,
        flags: MsgFlags,
    ) -> SysResult<i32> {
        let sock = self.any_socket(caller, fd)?;
        let vlen = vlen.min(UIO_MAXIOV);
        let mut sent = 0;
        while sent < vlen {
            let result = async {
                let at = offset(mmsg.addr(), sent as usize * wasixv2_abi::Mmsghdr::SIZE)?;
                let hdr: UserMsghdr = read_value(caller.memory(), GuestPtr::new(at)).await?;
                let n = self.send_msghdr(caller, &sock, &hdr, flags).await?;
                let len_at = GuestPtrMut::new(offset(at, UserMsghdr::SIZE)?);
                write_value(caller.memory(), len_at, &(n as u32)).await?;
                SysResult::Ok(())
            }
            .await;
            match result {
                Ok(()) => sent += 1,
                Err(_) if sent > 0 => break,
                Err(err) => return Err(err),
            }
        }
        Ok(sent as i32)
    }

    /// `do_recvmmsg`. `MSG_WAITFORONE` makes every message after the first non-blocking.
    // TODO: the timeout argument is not honoured yet.
    pub(crate) async fn sys_recvmmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        mmsg: MmsghdrArray,
        vlen: u32,
        flags: MsgFlags,
    ) -> SysResult<i32> {
        let sock = self.any_socket(caller, fd)?;
        let vlen = vlen.min(UIO_MAXIOV);
        let mut flags = flags;
        let mut received = 0;
        while received < vlen {
            let result = async {
                let at = offset(mmsg.addr(), received as usize * wasixv2_abi::Mmsghdr::SIZE)?;
                let n = self.recv_msghdr(caller, &sock, at, flags).await?;
                let len_at = GuestPtrMut::new(offset(at, UserMsghdr::SIZE)?);
                write_value(caller.memory(), len_at, &(n as u32)).await?;
                SysResult::Ok(())
            }
            .await;
            match result {
                Ok(()) => received += 1,
                Err(_) if received > 0 => break,
                Err(err) => return Err(err),
            }
            if flags.contains(MsgFlags::MSG_WAITFORONE) {
                flags |= MsgFlags::MSG_DONTWAIT;
            }
        }
        Ok(received as i32)
    }

    /// `__sys_shutdown` and `inet_shutdown`.
    pub(crate) async fn sys_shutdown(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        how: ShutdownHow,
    ) -> SysResult<i32> {
        let sock = match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => sock,
            AnySock::Unix(_, sock) => {
                sock.shutdown(unix::shutdown_mode(how.0)?);
                return Ok(0);
            }
            // `sock_no_shutdown`.
            AnySock::Netlink(..) | AnySock::Packet(..) => return Err(Errno::EOPNOTSUPP.into()),
        };
        let how = match how {
            ShutdownHow::SHUT_RD => Shutdown::Read,
            ShutdownHow::SHUT_WR => Shutdown::Write,
            ShutdownHow::SHUT_RDWR => Shutdown::Both,
            _ => return Err(Errno::EINVAL.into()),
        };
        sock.socket.shutdown(how)?;
        Ok(0)
    }

    /// `__sys_setsockopt`: the socket layer's own options for every family, the rest to the
    /// protocol.
    pub(crate) async fn sys_setsockopt(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        level: SocketLevel,
        name: i32,
        value: UserSlice,
    ) -> SysResult<i32> {
        let value = value.copy_in(caller.memory(), 4096).await?;
        let any = self.any_socket(caller, fd)?;
        if level == SocketLevel::SOL_SOCKET {
            match SocketOption(name) {
                SocketOption::SO_RCVTIMEO_OLD | SocketOption::SO_RCVTIMEO_NEW => {
                    any.timeouts().set(RCV, parse_timeout(name, &value)?);
                    return Ok(0);
                }
                SocketOption::SO_SNDTIMEO_OLD | SocketOption::SO_SNDTIMEO_NEW => {
                    any.timeouts().set(SND, parse_timeout(name, &value)?);
                    return Ok(0);
                }
                SocketOption::SO_TYPE
                | SocketOption::SO_PROTOCOL
                | SocketOption::SO_DOMAIN
                | SocketOption::SO_ERROR
                | SocketOption::SO_ACCEPTCONN
                | SocketOption::SO_PEERCRED => return Err(Errno::ENOPROTOOPT.into()),
                _ => {}
            }
        }
        match any {
            AnySock::Inet((_, sock)) => sock.socket.set_option(level.0, name, &value)?,
            // Unix sockets have no options of their own beyond `SOL_SOCKET`.
            AnySock::Unix(..) if level != SocketLevel::SOL_SOCKET => {
                return Err(Errno::EOPNOTSUPP.into());
            }
            AnySock::Unix(_, sock) => {
                let net_admin = self.cred(caller).capable(Cap::NetAdmin);
                sock.set_option(name, &value, net_admin)?
            }
            AnySock::Netlink(_, nl) if level == SocketLevel::SOL_SOCKET => {
                nl.set_option(name, &value)?
            }
            // `SOL_NETLINK` options (groups, strict checking) are accepted as no-ops.
            AnySock::Netlink(..) => {}
            // The `SOL_SOCKET` options that only describe the descriptor; `SOL_PACKET` ones
            // (memberships, rings, fanout) are about frames, which never come.
            AnySock::Packet(..) if level == SocketLevel::SOL_SOCKET => {}
            AnySock::Packet(..) => return Err(Errno::ENOPROTOOPT.into()),
        }
        Ok(0)
    }

    /// `do_sock_getsockopt`: values are truncated to the guest's `optlen`, and the length
    /// written is reported back.
    pub(crate) async fn sys_getsockopt(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        level: SocketLevel,
        name: i32,
        optval: Buf,
        optlen: SocklenPtr,
    ) -> SysResult<i32> {
        let any = self.any_socket(caller, fd)?;
        let len: i32 = read_value(caller.memory(), GuestPtr::new(optlen.addr())).await?;
        let len = usize::try_from(len).map_err(|_| Errno::EINVAL)?;
        if level == SocketLevel::SOL_SOCKET && SocketOption(name) == SocketOption::SO_PEERGROUPS {
            return peer_groups(caller, &any, len, optval, optlen).await;
        }
        let len = len.min(4096);
        let fixed = match &any {
            _ if level == SocketLevel::SOL_SOCKET && timeout_option(name).is_some() => {
                Some(timeout_value(any.timeouts(), name))
            }
            AnySock::Inet((_, sock)) => self.socket_option(sock, level, name)?,
            AnySock::Unix(_, sock) if level == SocketLevel::SOL_SOCKET => {
                Some(self.unix_getsockopt(sock, name)?)
            }
            AnySock::Netlink(_, nl) if level == SocketLevel::SOL_SOCKET => {
                Some(nl.getsockopt(name)?)
            }
            AnySock::Packet(_, p) if level == SocketLevel::SOL_SOCKET => Some(p.getsockopt(name)?),
            _ => return Err(Errno::EOPNOTSUPP.into()),
        };
        let (out, n) = match (fixed, &any) {
            (Some(value), _) => {
                let n = value.len().min(len);
                (value, n)
            }
            (None, AnySock::Inet((_, sock))) => {
                let mut out = vec![0; len];
                let n = sock.socket.get_option(level.0, name, &mut out)?;
                (out, n)
            }
            (None, _) => unreachable!("only IP sockets defer to the protocol"),
        };
        caller.memory().write(optval.addr(), &out[..n]).await?;
        write_value(caller.memory(), optlen, &(n as u32)).await?;
        Ok(0)
    }

    /// The `SOL_SOCKET` options the socket layer answers itself for an IP socket.
    fn socket_option(
        &self,
        sock: &SocketFile,
        level: SocketLevel,
        name: i32,
    ) -> Result<Option<Vec<u8>>, Errno> {
        if level != SocketLevel::SOL_SOCKET {
            return Ok(None);
        }
        let socket = &sock.socket;
        let int = |v: i32| Ok(Some(v.to_le_bytes().to_vec()));
        match SocketOption(name) {
            SocketOption::SO_TYPE => int(match socket.kind() {
                SocketKind::Stream => SocketType::SOCK_STREAM.0,
                SocketKind::Datagram => SocketType::SOCK_DGRAM.0,
                SocketKind::Raw => SocketType::SOCK_RAW.0,
            }),
            SocketOption::SO_PROTOCOL => int(socket.protocol()),
            SocketOption::SO_DOMAIN => int(match socket.domain() {
                Domain::Inet => AddressFamily::AF_INET.0,
                Domain::Inet6 => AddressFamily::AF_INET6.0,
            }),
            SocketOption::SO_ERROR => int(socket.take_error().map_or(0, |errno| errno.0)),
            // `cred_to_ucred` without peer credentials, which only Unix sockets have.
            SocketOption::SO_PEERCRED => Ok(Some(unix::Ucred::NONE.to_bytes())),
            _ => Ok(None),
        }
    }

    /// The call context for an operation on `file`.
    pub(crate) fn io<'a>(
        caller: &Caller<'_>,
        interrupt: &'a dyn wasixv2_wait::Interrupt,
        file: &OpenFile,
    ) -> crate::file::Io<'a> {
        crate::file::Io {
            task: caller.task(),
            flags: file.flags(),
            interrupt,
            broken: None,
        }
    }
}

/// `SO_PEERGROUPS`: the supplementary groups of a socket's peer, `ENODATA` without one (only
/// Unix sockets have peer credentials); a buffer too short is `ERANGE` with the length it
/// needs written back.
async fn peer_groups(
    caller: &Caller<'_>,
    any: &AnySock,
    len: usize,
    optval: Buf,
    optlen: SocklenPtr,
) -> SysResult<i32> {
    let peer = match any {
        AnySock::Unix(_, sock) => sock.peer_cred(),
        _ => None,
    };
    let groups = peer.ok_or(Errno::ENODATA)?.groups;
    let need = groups.len() * 4;
    if len < need {
        write_value(caller.memory(), optlen, &(need as u32)).await?;
        return Err(Errno::ERANGE.into());
    }
    let raw: Vec<u8> = groups.iter().flat_map(|g| g.to_le_bytes()).collect();
    caller.memory().write(optval.addr(), &raw).await?;
    write_value(caller.memory(), optlen, &(need as u32)).await?;
    Ok(0)
}

/// An open file for the Unix socket `sock`.
fn unix_file(sock: Arc<unix::Sock>, flags: SocketFlags, path: Path) -> OpenFile {
    sock.graft(path.dentry.node().ino());
    let mut status = OpenFlags::O_RDWR;
    if flags.contains(SocketFlags::SOCK_NONBLOCK) {
        status |= OpenFlags::O_NONBLOCK;
    }
    OpenFile::new(Backing::Stream(Arc::new(UnixFile(sock)), path), status)
}

/// Splits `socket`'s type argument into the type and the `SOCK_NONBLOCK`/`SOCK_CLOEXEC` flags.
fn socket_type(r#type: i32) -> Result<(SocketType, SocketFlags), Errno> {
    let flags = SocketFlags((r#type & !SOCK_TYPE_MASK) as u32);
    if !(flags & !(SocketFlags::SOCK_CLOEXEC | SocketFlags::SOCK_NONBLOCK)).is_empty() {
        return Err(Errno::EINVAL);
    }
    let kind = r#type & SOCK_TYPE_MASK;
    if !(0..SOCK_MAX).contains(&kind) {
        return Err(Errno::EINVAL);
    }
    Ok((SocketType(kind), flags))
}

/// What a Unix receive returns: a record's full length with `MSG_TRUNC`.
fn unix_result(sock: &unix::Sock, flags: MsgFlags, msg: &unix::Received) -> usize {
    match sock.kind() != unix::Kind::Stream && flags.contains(MsgFlags::MSG_TRUNC) {
        true => msg.full_len,
        false => msg.len(),
    }
}

/// What a receive returns: the datagram's full length with `MSG_TRUNC`.
fn recv_result(sock: &SocketFile, flags: MsgFlags, msg: &wasixv2_vnet::RecvMsg) -> usize {
    let datagram = sock.socket.kind() == SocketKind::Datagram;
    match datagram && flags.contains(MsgFlags::MSG_TRUNC) {
        true => msg.full_len,
        false => msg.len,
    }
}

/// `base + off` as a guest address; `EFAULT` past the end of the address space.
pub(crate) fn offset(base: u32, off: usize) -> Result<u32, Errno> {
    u32::try_from(off)
        .ok()
        .and_then(|off| base.checked_add(off))
        .ok_or(Errno::EFAULT)
}

/// A guest buffer as the iovecs it is made of (`iov_iter`), copied in and out in pieces.
struct GuestIov {
    iovecs: Vec<Iovec>,
    len: usize,
}

impl GuestIov {
    /// One buffer, as `import_ubuf` takes it: at most `MAX_RW_COUNT` bytes.
    fn single(addr: u32, len: u32) -> Self {
        let len = len.min(MAX_RW_COUNT);
        Self {
            iovecs: vec![Iovec {
                iov_base: addr,
                iov_len: len,
            }],
            len: len as usize,
        }
    }

    /// A msghdr's iovecs (`import_iovec`), whose total `read_iovecs` bounds.
    async fn of(memory: &dyn UserMemory, hdr: &UserMsghdr) -> SysResult<Self> {
        let iovecs = read_iovecs(memory, GuestPtr::new(hdr.msg_iov.addr()), hdr.msg_iovlen).await?;
        let len = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        Ok(Self { iovecs, len })
    }

    /// The guest ranges making up `[at, at + len)`.
    fn ranges(&self, at: usize, len: usize) -> impl Iterator<Item = Result<(u32, usize), Errno>> {
        let mut skip = at;
        let mut left = len;
        self.iovecs.iter().filter_map(move |iov| {
            let iov_len = iov.iov_len as usize;
            if left == 0 {
                return None;
            }
            if skip >= iov_len {
                skip -= iov_len;
                return None;
            }
            let n = (iov_len - skip).min(left);
            let addr = offset(iov.iov_base, skip);
            skip = 0;
            left -= n;
            Some(addr.map(|addr| (addr, n)))
        })
    }

    /// Copies the guest bytes at `[at, at + out.len())` into `out`.
    async fn copy_in(&self, memory: &dyn UserMemory, at: usize, out: &mut [u8]) -> SysResult<()> {
        let mut done = 0;
        for range in self.ranges(at, out.len()) {
            let (addr, n) = range?;
            memory.read(addr, &mut out[done..done + n]).await?;
            done += n;
        }
        Ok(())
    }

    /// Copies `data` to the guest bytes at `[at, at + data.len())`.
    async fn copy_out(&self, memory: &dyn UserMemory, at: usize, data: &[u8]) -> SysResult<()> {
        let mut done = 0;
        for range in self.ranges(at, data.len()) {
            let (addr, n) = range?;
            memory.write(addr, &data[done..done + n]).await?;
            done += n;
        }
        Ok(())
    }
}

/// The receive flags under which a TCP read is not one that takes only what reached the user:
/// `MSG_PEEK` leaves the data anyway, `MSG_TRUNC` discards it uncopied, and `MSG_OOB` and
/// `MSG_ERRQUEUE` read other things.
const TCP_OTHER_READS: MsgFlags = MsgFlags(
    MsgFlags::MSG_PEEK.0 | MsgFlags::MSG_TRUNC.0 | MsgFlags::MSG_OOB.0 | MsgFlags::MSG_ERRQUEUE.0,
);

/// The guest buffers of a receive from byte `at` on, as a [`ReadSink`].
struct IovOut<'a> {
    memory: &'a dyn UserMemory,
    iov: &'a GuestIov,
    at: usize,
}

impl<'a> IovOut<'a> {
    fn new(memory: &'a dyn UserMemory, iov: &'a GuestIov, at: usize) -> Self {
        Self { memory, iov, at }
    }
}

impl ReadSink for IovOut<'_> {
    fn copy_out<'a>(&'a mut self, data: &'a [u8]) -> LocalFuture<'a, usize> {
        Box::pin(async move {
            let mut done = 0;
            for range in self.iov.ranges(self.at, data.len()) {
                let Ok((addr, n)) = range else { break };
                if self
                    .memory
                    .write(addr, &data[done..done + n])
                    .await
                    .is_err()
                {
                    break;
                }
                done += n;
            }
            self.at += done;
            done
        })
    }
}

thread_local! {
    /// Bounce buffers for socket I/O. Guest memory is reached asynchronously while sockets
    /// work on byte slices, so data passes through kernel memory; reusing buffers spares
    /// allocating and zeroing one per call. What a buffer held before is never shown to a
    /// guest: a send fills it from guest memory first, a receive copies out only what the
    /// socket wrote. The pool is per host thread, so machines in one process share no lock.
    static SCRATCH: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}
/// How many idle buffers a thread's pool keeps.
const SCRATCH_KEEP: usize = 4;

#[derive(Default)]
struct Scratch(Vec<u8>);

impl Scratch {
    /// A buffer of `len` bytes.
    fn get(&mut self, len: usize) -> &mut [u8] {
        if self.0.capacity() == 0 {
            self.0 = SCRATCH
                .with_borrow_mut(|pool| pool.pop())
                .unwrap_or_default();
        }
        if self.0.len() < len {
            self.0.resize(len, 0);
        }
        &mut self.0[..len]
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let buf = std::mem::take(&mut self.0);
        if buf.capacity() == 0 || buf.len() > SOCK_CHUNK {
            return;
        }
        // `try_with`: a thread being torn down keeps nothing.
        let _ = SCRATCH.try_with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.len() < SCRATCH_KEEP {
                pool.push(buf);
            }
        });
    }
}

/// `move_addr_to_kernel`.
async fn move_addr_to_kernel(
    caller: &Caller<'_>,
    addr: ConstSockaddrPtr,
    len: Socklen,
) -> SysResult<Vec<u8>> {
    if len > SOCKADDR_STORAGE {
        return Err(Errno::EINVAL.into());
    }
    let mut raw = vec![0; len as usize];
    if len > 0 {
        caller.memory().read(addr.addr(), &mut raw).await?;
    }
    Ok(raw)
}

/// `move_addr_to_user`: copies as much of `raw` as the guest's length allows and reports the
/// real length.
async fn move_addr_to_user(
    caller: &Caller<'_>,
    raw: &[u8],
    addr: SockaddrPtr,
    addrlen: SocklenPtr,
) -> SysResult<()> {
    let memory = caller.memory();
    let len: i32 = read_value(memory, GuestPtr::new(addrlen.addr())).await?;
    let len = (len as i64).min(raw.len() as i64);
    if len < 0 {
        return Err(Errno::EINVAL.into());
    }
    if len > 0 {
        memory.write(addr.addr(), &raw[..len as usize]).await?;
    }
    write_value(memory, addrlen, &(raw.len() as u32)).await?;
    Ok(())
}

/// `msg_namelen`, clamped as `__copy_msghdr` does.
fn msg_namelen(hdr: &UserMsghdr) -> Result<u32, Errno> {
    if hdr.msg_name.is_null() {
        return Ok(0);
    }
    let len = u32::try_from(hdr.msg_namelen).map_err(|_| Errno::EINVAL)?;
    Ok(len.min(SOCKADDR_STORAGE))
}

/// The control messages of a send, each as it is; the protocol interprets them. Lengths past
/// `INT_MAX` or `optmem_max` are `ENOBUFS`, as `____sys_sendmsg` and `sock_kmalloc` decide.
async fn read_cmsgs(caller: &Caller<'_>, hdr: &UserMsghdr) -> SysResult<Vec<Control>> {
    let len = hdr.msg_controllen as usize;
    if hdr.msg_control.is_null() || len == 0 {
        return Ok(Vec::new());
    }
    if len > i32::MAX as usize || len > OPTMEM_MAX {
        return Err(Errno::ENOBUFS.into());
    }
    let mut raw = vec![0; len];
    caller
        .memory()
        .read(hdr.msg_control.addr(), &mut raw)
        .await?;
    Ok(parse_cmsgs(&raw)?)
}

/// `__sock_cmsg_send` for an IP socket: checks the `SOL_SOCKET` control messages every family
/// shares and returns the others, which are the protocol's. `SCM_RIGHTS` and `SCM_CREDENTIALS`
/// belong to Unix sockets and are ignored. What they ask for (a mark, a priority, transmit
/// timestamps) has no effect without packets.
fn sock_cmsg_send(cred: &Cred, controls: &[Control]) -> Result<Vec<Control>, Errno> {
    let privileged = cred.capable(Cap::NetRaw) || cred.capable(Cap::NetAdmin);
    let mut rest = Vec::new();
    for c in controls {
        if c.level != SocketLevel::SOL_SOCKET.0 {
            rest.push(c.clone());
            continue;
        }
        let word = <[u8; 4]>::try_from(c.data.as_slice()).map(u32::from_le_bytes);
        match c.kind {
            unix::SCM_RIGHTS | unix::SCM_CREDENTIALS => {}
            SO_MARK => {
                if !privileged {
                    return Err(Errno::EPERM);
                }
                word.map_err(|_| Errno::EINVAL)?;
            }
            SO_TIMESTAMPING_OLD | SO_TIMESTAMPING_NEW => {
                let flags = word.map_err(|_| Errno::EINVAL)?;
                if flags & !SOF_TIMESTAMPING_TX_RECORD_MASK != 0 {
                    return Err(Errno::EINVAL);
                }
            }
            SO_PRIORITY => {
                // `sk_set_prio_allowed`: `TC_PRIO_BESTEFFORT` to `TC_PRIO_INTERACTIVE` for all.
                let prio = word.map_err(|_| Errno::EINVAL)?;
                if prio > 6 && !privileged {
                    return Err(Errno::EPERM);
                }
            }
            SCM_DEVMEM_DMABUF => {
                word.map_err(|_| Errno::EINVAL)?;
            }
            // `SCM_TXTIME` needs `SO_TXTIME` and `SCM_TS_OPT_ID` `SOF_TIMESTAMPING_OPT_ID`
            // (and no TCP socket), options no socket here can have; the rest are unknown.
            _ => return Err(Errno::EINVAL),
        }
    }
    Ok(rest)
}

/// The `SOL_SOCKET` control message types of the generic socket ABI.
const SO_PRIORITY: i32 = 12;
const SO_MARK: i32 = 36;
const SO_TIMESTAMPING_OLD: i32 = 37;
const SO_TIMESTAMPING_NEW: i32 = 65;
const SCM_DEVMEM_DMABUF: i32 = 79;
/// `SOF_TIMESTAMPING_TX_RECORD_MASK`: the transmit timestamp requests a send may make.
const SOF_TIMESTAMPING_TX_RECORD_MASK: u32 = (1 << 0) | (1 << 1) | (1 << 8) | (1 << 9) | (1 << 18);

/// `for_each_cmsghdr` with `CMSG_OK`: each header must fit with its `cmsg_len` inside the
/// buffer (else `EINVAL`), and the walk ends once the next aligned header would not fit
/// (`__cmsg_nxthdr`).
fn parse_cmsgs(raw: &[u8]) -> Result<Vec<Control>, Errno> {
    let mut controls = Vec::new();
    let mut at = 0;
    while let Some(rest) = raw.get(at..).filter(|rest| rest.len() >= CMSG_HDR) {
        let word = |i: usize| u32::from_le_bytes(rest[i..i + 4].try_into().unwrap());
        let cmsg_len = word(0) as usize;
        if cmsg_len < CMSG_HDR || cmsg_len > rest.len() {
            return Err(Errno::EINVAL);
        }
        controls.push(Control {
            level: word(4) as i32,
            kind: word(8) as i32,
            data: rest[CMSG_HDR..cmsg_len].to_vec(),
        });
        at += cmsg_len.next_multiple_of(CMSG_ALIGN);
    }
    Ok(controls)
}

/// `put_cmsg` for each control message; sets `MSG_CTRUNC` for what does not fit and returns
/// the bytes used.
async fn write_cmsgs(
    caller: &Caller<'_>,
    hdr: &UserMsghdr,
    controls: &[Control],
    flags: &mut MsgFlags,
) -> SysResult<usize> {
    let space = if hdr.msg_control.is_null() {
        0
    } else {
        hdr.msg_controllen as usize
    };
    let mut used = 0;
    for control in controls {
        let left = space - used;
        if left < CMSG_HDR {
            *flags |= MsgFlags::MSG_CTRUNC;
            break;
        }
        let full = CMSG_HDR + control.data.len();
        let len = full.min(left);
        if len < full {
            *flags |= MsgFlags::MSG_CTRUNC;
        }
        let mut raw = Vec::with_capacity(len);
        raw.extend((len as u32).to_le_bytes());
        raw.extend(control.level.to_le_bytes());
        raw.extend(control.kind.to_le_bytes());
        raw.extend(&control.data[..len - CMSG_HDR]);
        let at = offset(hdr.msg_control.addr(), used)?;
        caller.memory().write(at, &raw).await?;
        used += full.next_multiple_of(CMSG_ALIGN).min(left);
    }
    Ok(used)
}

/// `SO_RCVTIMEO*` and `SO_SNDTIMEO*`: the direction, and whether the value is the old 32-bit
/// `timeval`.
fn timeout_option(name: i32) -> Option<(usize, bool)> {
    match SocketOption(name) {
        SocketOption::SO_RCVTIMEO_OLD => Some((RCV, true)),
        SocketOption::SO_RCVTIMEO_NEW => Some((RCV, false)),
        SocketOption::SO_SNDTIMEO_OLD => Some((SND, true)),
        SocketOption::SO_SNDTIMEO_NEW => Some((SND, false)),
        _ => None,
    }
}

/// `sock_get_timeout`: the timeout as the guest's `timeval`, zero for none.
fn timeout_value(timeouts: &Timeouts, name: i32) -> Vec<u8> {
    let (dir, old) = timeout_option(name).expect("a timeout option");
    let timeout = timeouts.get(dir).unwrap_or_default();
    let (sec, usec) = (timeout.as_secs() as i64, timeout.subsec_micros() as i64);
    if old {
        let tv = KernelOldTimeval {
            tv_sec: sec as i32,
            tv_usec: usec as i32,
        };
        let mut raw = vec![0; KernelOldTimeval::SIZE];
        tv.write_to(&mut raw);
        raw
    } else {
        let tv = KernelSockTimeval {
            tv_sec: sec,
            tv_usec: usec,
        };
        let mut raw = vec![0; KernelSockTimeval::SIZE];
        tv.write_to(&mut raw);
        raw
    }
}

/// `sock_set_timeout`: an all-zero value means no timeout, a negative one an immediate one;
/// microseconds out of range are `EDOM`.
fn parse_timeout(name: i32, raw: &[u8]) -> Result<Option<Duration>, Errno> {
    let old = matches!(
        SocketOption(name),
        SocketOption::SO_RCVTIMEO_OLD | SocketOption::SO_SNDTIMEO_OLD
    );
    let (sec, usec) = if old {
        let tv = KernelOldTimeval::read_from(raw).ok_or(Errno::EINVAL)?;
        (i64::from(tv.tv_sec), i64::from(tv.tv_usec))
    } else {
        let tv = KernelSockTimeval::read_from(raw).ok_or(Errno::EINVAL)?;
        (tv.tv_sec, tv.tv_usec)
    };
    if !(0..1_000_000).contains(&usec) {
        return Err(Errno::EDOM);
    }
    if sec < 0 {
        return Ok(Some(Duration::ZERO));
    }
    if sec == 0 && usec == 0 {
        return Ok(None);
    }
    Ok(Some(Duration::new(sec as u64, usec as u32 * 1000)))
}

/// The sender of a netlink request, for whom `IFLA_NET_NS_PID` and `IFLA_NET_NS_FD` name
/// namespaces.
struct NetlinkSender<'a> {
    kernel: &'a Kernel,
    caller: &'a Caller<'a>,
}

impl netlink::Sender for NetlinkSender<'_> {
    fn net_of_pid(&self, pid: u32) -> Result<Arc<NetNs>, Errno> {
        let pid = i32::try_from(pid).map_err(|_| Errno::ESRCH)?;
        let process = self.kernel.lookup_pid(self.caller, pid)?;
        self.kernel.net_ns_of(&process).ok_or(Errno::ESRCH)
    }

    fn net_of_fd(&self, fd: i32) -> Result<Arc<NetNs>, Errno> {
        let file = self.kernel.io_file(self.caller, Fd(fd))?;
        match file
            .downcast::<crate::nsfs::NsFile>()
            .map(|f| f.namespace())
        {
            Some(crate::ns::Namespace::Net(ns)) => Ok(ns.clone()),
            _ => Err(Errno::EINVAL),
        }
    }
}

impl Kernel {
    /// The network namespace a socket was made in (`sock_net`); `None` if `file` is not a
    /// socket.
    pub(crate) fn socket_net(&self, file: &OpenFile) -> Option<Arc<NetNs>> {
        if let Some(socket) = file.downcast::<SocketFile>() {
            return Some(socket.net.clone());
        }
        if let Some(unix) = file.downcast::<UnixFile>() {
            return Some(unix.0.net().clone());
        }
        if let Some(p) = file.object().cloned().and_then(packet::downcast) {
            return Some(p.net.clone());
        }
        let netlink = file.object().cloned().and_then(netlink::downcast)?;
        Some(netlink.net.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmsg(len: u32, level: i32, kind: i32, data: &[u8]) -> Vec<u8> {
        let mut raw = len.to_le_bytes().to_vec();
        raw.extend(level.to_le_bytes());
        raw.extend(kind.to_le_bytes());
        raw.extend(data);
        raw
    }

    #[test]
    fn cmsg_walk_stops_at_the_end_of_an_unaligned_buffer() {
        // `msg_controllen` 13 with `cmsg_len` 13: one byte of data, then the end.
        let raw = cmsg(13, 1, 1, &[7]);
        let controls = parse_cmsgs(&raw).unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].data, [7]);
        // Two messages, the second unaligned at the very end.
        let mut raw = cmsg(16, 1, 1, &[1, 2, 3, 4]);
        raw.extend(cmsg(14, 0, 2, &[5, 6]));
        let controls = parse_cmsgs(&raw).unwrap();
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[1].data, [5, 6]);
        // A trailing piece too short for a header is ignored.
        let mut raw = cmsg(12, 1, 1, &[]);
        raw.extend([0xff; 11]);
        assert_eq!(parse_cmsgs(&raw).unwrap().len(), 1);
    }

    #[test]
    fn bad_cmsg_lengths_are_einval() {
        assert_eq!(parse_cmsgs(&cmsg(11, 1, 1, &[])), Err(Errno::EINVAL));
        assert_eq!(parse_cmsgs(&cmsg(17, 1, 1, &[0; 4])), Err(Errno::EINVAL));
        assert_eq!(
            parse_cmsgs(&cmsg(u32::MAX, 1, 1, &[0; 4])),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn cmsg_parsing_never_panics() {
        for seed in 0..5000u32 {
            let len = (seed % 61) as usize;
            let raw: Vec<u8> = (0..len)
                .map(|i| {
                    let x = seed.wrapping_mul(2_654_435_761) ^ (i as u32).wrapping_mul(40_503);
                    // Small values often, so lengths land near the buffer's end.
                    if i % 12 == 0 {
                        (x % 32) as u8
                    } else {
                        (x >> 9) as u8
                    }
                })
                .collect();
            let _ = parse_cmsgs(&raw);
        }
    }

    #[test]
    fn iovec_ranges_skip_and_split() {
        let iov = GuestIov {
            iovecs: vec![
                Iovec {
                    iov_base: 100,
                    iov_len: 4,
                },
                Iovec {
                    iov_base: 200,
                    iov_len: 0,
                },
                Iovec {
                    iov_base: 300,
                    iov_len: 8,
                },
            ],
            len: 12,
        };
        let ranges: Vec<_> = iov.ranges(2, 6).map(Result::unwrap).collect();
        assert_eq!(ranges, [(102, 2), (300, 4)]);
        let past_end = GuestIov::single(u32::MAX - 1, 8);
        let mut ranges = past_end.ranges(4, 2);
        assert_eq!(ranges.next(), Some(Err(Errno::EFAULT)));
        assert_eq!(offset(u32::MAX, 1), Err(Errno::EFAULT));
    }
}
