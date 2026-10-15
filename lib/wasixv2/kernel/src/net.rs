//! The socket syscalls, as Linux's `net/socket.c`: descriptor handling, guest struct decoding
//! (`move_addr_to_kernel`, `move_addr_to_user`, msghdr and cmsg layouts), blocking and the
//! `SOL_SOCKET` options the socket layer itself answers. IP protocol behaviour comes from the
//! network namespace (`wasixv2_vnet::Network`); Unix sockets are not implemented yet.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::{
    AddressFamily, Buf, ConstSockaddrPtr, ConstUserMsghdrPtr, FdPair, GuestLayout, GuestPtr,
    GuestPtrMut, IoctlRequest, Iovec, KernelOldTimeval, KernelSockTimeval, MmsghdrArray, MsgFlags,
    OpenFlags, ShutdownHow, SockaddrPtr, SocketFlags, SocketLevel, SocketOption, SocketType,
    Socklen, SocklenPtr, UserMsghdr, UserMsghdrPtr,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, SysErr, SysResult, UserSlice, UserSliceMut, read_value, write_value,
};
use wasixv2_vnet::{Control, Domain, SendMsg, Shutdown, SocketKind};

use crate::{
    IO_CHUNK, Kernel,
    file::{Backing, OpenFile},
    fs::read_iovecs,
    netlink,
    socket::{self, RCV, SND, SOCKADDR_STORAGE, SocketFile},
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
/// `sizeof(struct cmsghdr)` and its alignment on wasm32.
const CMSG_HDR: usize = 12;
const CMSG_ALIGN: usize = 4;

/// An open socket: its open file and its socket state.
type Sock = (Arc<OpenFile>, Arc<SocketFile>);

/// An open socket of any family the kernel implements.
enum AnySock {
    Inet(Sock),
    Unix(Arc<OpenFile>, Arc<unix::Sock>),
    Netlink(Arc<OpenFile>, Arc<netlink::NetlinkSocket>),
}

/// What one receive produced, for `recv_msghdr`.
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
        self.socket_fd(caller, fd).map(AnySock::Inet)
    }

    fn install_unix(&self, caller: &Caller<'_>, sock: Arc<unix::Sock>, flags: SocketFlags) -> Fd {
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = OpenFile::new(Backing::Stream(Arc::new(UnixFile(sock))), status);
        let [fd] = self.install_files(caller, [file], flags.contains(SocketFlags::SOCK_CLOEXEC));
        fd
    }

    /// `sockfd_lookup`: `EBADF`, then `ENOTSOCK` for a descriptor that is not a socket.
    fn socket_fd(&self, caller: &Caller<'_>, fd: Fd) -> SysResult<Sock> {
        let file = self.file(caller.task(), fd)?;
        let object = file.object().ok_or(Errno::ENOTSOCK)?.clone();
        let any: Arc<dyn std::any::Any + Send + Sync> = object;
        let socket = any.downcast::<SocketFile>().map_err(|_| Errno::ENOTSOCK)?;
        Ok((file, socket))
    }

    fn install_socket(
        &self,
        caller: &Caller<'_>,
        socket: Arc<dyn wasixv2_vnet::Socket>,
        flags: SocketFlags,
    ) -> Fd {
        let mut status = OpenFlags::O_RDWR;
        if flags.contains(SocketFlags::SOCK_NONBLOCK) {
            status |= OpenFlags::O_NONBLOCK;
        }
        let object = SocketFile::new(socket, self.host.clone());
        let file = OpenFile::new(Backing::Stream(Arc::new(object)), status);
        let cloexec = flags.contains(SocketFlags::SOCK_CLOEXEC);
        let [fd] = self.install_files(caller, [file], cloexec);
        fd
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
                return Ok(self.install_unix(caller, sock, flags));
            }
            AddressFamily::AF_NETLINK => return self.netlink_socket(caller, kind, protocol, flags),
            // Every other family (and anything past NPROTO) has no implementation.
            _ => return Err(Errno::EAFNOSUPPORT.into()),
        };
        let kind = match kind {
            SocketType::SOCK_STREAM => SocketKind::Stream,
            SocketType::SOCK_DGRAM => SocketKind::Datagram,
            // `inet_create` matches raw sockets, then wants CAP_NET_RAW.
            SocketType::SOCK_RAW => return Err(Errno::EPERM.into()),
            _ => return Err(Errno::ESOCKTNOSUPPORT.into()),
        };
        let network = self.network.as_ref().ok_or(Errno::EAFNOSUPPORT)?;
        let socket = network.socket(domain, kind, protocol)?;
        Ok(self.install_socket(caller, socket, flags))
    }

    /// `__sys_socketpair`. Only Unix sockets have pairs, and they do not exist yet.
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
        let fds = [
            self.install_unix(caller, a, flags),
            self.install_unix(caller, b, flags),
        ];
        self.copy_out_fd_pair(caller, usockvec, fds).await?;
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
        };
        let domain = sock.socket.domain();
        let addr = socket::decode(domain, &raw, true)?;
        // AF_UNSPEC is accepted for IPv4 only with INADDR_ANY.
        if socket::family(&raw)? == AddressFamily::AF_UNSPEC && !addr.ip().is_unspecified() {
            return Err(Errno::EAFNOSUPPORT.into());
        }
        // Processes have no capabilities, so no CAP_NET_BIND_SERVICE.
        sock.socket.bind(addr, false)?;
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
            AnySock::Unix(_, sock) => sock.listen(backlog)?,
            AnySock::Netlink(..) => return Err(Errno::EOPNOTSUPP.into()),
        }
        Ok(0)
    }

    /// `do_accept` and `inet_accept`: waits for a connection unless non-blocking.
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
        let (file, sock) = match self.any_socket(caller, fd)? {
            AnySock::Inet(sock) => sock,
            AnySock::Unix(file, sock) => {
                let accepted = self.unix_accept(caller, &file, &sock).await?;
                if !addr.is_null() {
                    let raw = accepted.peer_addr().unwrap_or_default();
                    move_addr_to_user(caller, &raw, addr, addrlen).await?;
                }
                return Ok(self.install_unix(caller, accepted, flags));
            }
            AnySock::Netlink(..) => return Err(Errno::EOPNOTSUPP.into()),
        };
        let interrupt = self.interrupt(caller);
        let io = Self::io(&*interrupt, &file);
        let accepted = sock
            .block(io, RCV, wasixv2_wait::PollMask::READABLE, || {
                sock.socket.accept()
            })
            .await?;
        if !addr.is_null() {
            let raw = socket::encode(accepted.peer);
            move_addr_to_user(caller, &raw, addr, addrlen).await?;
        }
        Ok(self.install_socket(caller, accepted.socket, flags))
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
            AnySock::Netlink(..) => return Ok(0),
        };
        if socket::family(&raw)? == AddressFamily::AF_UNSPEC {
            sock.socket.disconnect()?;
            return Ok(0);
        }
        let socket = &sock.socket;
        let addr = socket::decode(socket.domain(), &raw, false)?;
        match socket.connect(addr) {
            Err(Errno::EINPROGRESS) => {}
            result => return Ok(result.map(|()| 0)?),
        }
        let interrupt = self.interrupt(caller);
        let io = Self::io(&*interrupt, &file);
        if io.nonblocking() {
            return Err(Errno::EINPROGRESS.into());
        }
        let finished = || match socket.take_error() {
            Some(errno) => Err(errno),
            None if socket.peer_addr().is_ok() => Ok(()),
            None => Err(Errno::EAGAIN),
        };
        match sock
            .block(io, SND, wasixv2_wait::PollMask::WRITABLE, finished)
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
            AnySock::Netlink(..) => netlink::kernel_addr(),
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
        let (file, sock) = match self.any_socket(caller, fd)? {
            AnySock::Inet(sock) => sock,
            AnySock::Unix(file, sock) => {
                let raw = match addr.is_null() {
                    true => None,
                    false => Some(move_addr_to_kernel(caller, addr, len).await?),
                };
                let to = self.unix_destination(caller, &sock, raw.as_deref()).await?;
                let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
                let interrupt = self.interrupt(caller);
                let io = dont_wait(Self::io(&*interrupt, &file), flags);
                let cred = self.ucred(caller);
                let anc = unix::Ancillary::default();
                return Ok(unix::send(&sock, io, &data, to, anc, cred).await? as u32);
            }
            AnySock::Netlink(_, nl) => {
                let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
                return Ok(nl.send(&data)? as u32);
            }
        };
        let to = match addr.is_null() {
            true => None,
            false => {
                let raw = move_addr_to_kernel(caller, addr, len).await?;
                self.destination(&sock, &raw)?
            }
        };
        let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
        let msg = SendMsg {
            data: &data,
            to,
            flags,
            control: &[],
        };
        Ok(sock
            .send(Self::io(&*self.interrupt(caller), &file), msg)
            .await? as u32)
    }

    /// A send's destination. Stream sockets ignore it, as TCP does.
    fn destination(&self, sock: &SocketFile, raw: &[u8]) -> Result<Option<SocketAddr>, Errno> {
        if sock.socket.kind() == SocketKind::Stream {
            return Ok(None);
        }
        socket::decode(sock.socket.domain(), raw, true).map(Some)
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
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let (file, sock) = match self.any_socket(caller, fd)? {
            AnySock::Inet(sock) => sock,
            AnySock::Unix(file, sock) => {
                let msg = unix::recv(
                    &sock,
                    Self::io(&*self.interrupt(caller), &file),
                    &mut data,
                    flags,
                )
                .await?;
                buf.copy_out(caller.memory(), &data[..msg.len]).await?;
                if !addr.is_null() {
                    let raw = msg.from.clone().unwrap_or_default();
                    move_addr_to_user(caller, &raw, addr, addrlen).await?;
                }
                return Ok(unix_result(&sock, flags, &msg) as u32);
            }
            AnySock::Netlink(file, nl) => {
                let interrupt = self.interrupt(caller);
                let io = dont_wait(Self::io(&*interrupt, &file), flags);
                let (len, full) = nl.recv(io, &mut data, flags).await?;
                buf.copy_out(caller.memory(), &data[..len]).await?;
                if !addr.is_null() {
                    move_addr_to_user(caller, &netlink::kernel_addr(), addr, addrlen).await?;
                }
                let truncated = flags.contains(MsgFlags::MSG_TRUNC);
                return Ok(if truncated { full } else { len } as u32);
            }
        };
        let msg = sock
            .recv(Self::io(&*self.interrupt(caller), &file), &mut data, flags)
            .await?;
        buf.copy_out(caller.memory(), &data[..msg.len]).await?;
        if !addr.is_null() {
            let raw = msg.from.map(socket::encode).unwrap_or_default();
            move_addr_to_user(caller, &raw, addr, addrlen).await?;
        }
        Ok(recv_result(&sock, flags, &msg) as u32)
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
        let memory = caller.memory();
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
        let control = read_cmsgs(caller, hdr).await?;
        let mut data = Vec::new();
        for iov in read_iovecs(memory, GuestPtr::new(hdr.msg_iov.addr()), hdr.msg_iovlen).await? {
            let slice = UserSlice::new(iov.iov_base, iov.iov_len);
            data.extend(slice.copy_in(memory, IO_CHUNK - data.len()).await?);
        }
        match sock {
            AnySock::Inet((file, sock)) => {
                // `SCM_RIGHTS` and `SCM_CREDENTIALS` only work on Unix sockets.
                if control.iter().any(|c| {
                    c.level == SocketLevel::SOL_SOCKET.0
                        && matches!(c.kind, unix::SCM_RIGHTS | unix::SCM_CREDENTIALS)
                }) {
                    return Err(Errno::EINVAL.into());
                }
                let to = match &raw_to {
                    Some(raw) => self.destination(sock, raw)?,
                    None => None,
                };
                let msg = SendMsg {
                    data: &data,
                    to,
                    flags,
                    control: &control,
                };
                sock.send(Self::io(&*self.interrupt(caller), file), msg)
                    .await
            }
            AnySock::Unix(file, sock) => {
                let to = self
                    .unix_destination(caller, sock, raw_to.as_deref())
                    .await?;
                let anc = self.unix_ancillary(caller, &control)?;
                let interrupt = self.interrupt(caller);
                let io = dont_wait(Self::io(&*interrupt, file), flags);
                let cred = self.ucred(caller);
                unix::send(sock, io, &data, to, anc, cred).await
            }
            AnySock::Netlink(_, nl) => Ok(nl.send(&data)?),
        }
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
        let iovecs = read_iovecs(memory, GuestPtr::new(hdr.msg_iov.addr()), hdr.msg_iovlen).await?;
        let total: usize = iovecs.iter().map(|iov| iov.iov_len as usize).sum();
        let mut data = vec![0; total.min(IO_CHUNK)];
        let got = match sock {
            AnySock::Inet((file, sock)) => {
                let msg = sock
                    .recv(Self::io(&*self.interrupt(caller), file), &mut data, flags)
                    .await?;
                Got {
                    len: msg.len,
                    result: recv_result(sock, flags, &msg),
                    from: msg.from.map(socket::encode),
                    flags: msg.flags,
                    controls: msg.control,
                    rights: Vec::new(),
                }
            }
            AnySock::Unix(file, sock) => {
                let msg = unix::recv(
                    sock,
                    Self::io(&*self.interrupt(caller), file),
                    &mut data,
                    flags,
                )
                .await?;
                let controls = msg
                    .cred
                    .map(|cred| Control {
                        level: SocketLevel::SOL_SOCKET.0,
                        kind: unix::SCM_CREDENTIALS,
                        data: cred.to_bytes(),
                    })
                    .into_iter()
                    .collect();
                Got {
                    len: msg.len,
                    result: unix_result(sock, flags, &msg),
                    // An unnamed sender has an empty address.
                    from: Some(msg.from.unwrap_or_default()),
                    flags: msg.flags,
                    controls,
                    rights: msg.rights,
                }
            }
            AnySock::Netlink(file, nl) => {
                let interrupt = self.interrupt(caller);
                let io = dont_wait(Self::io(&*interrupt, file), flags);
                let (len, full) = nl.recv(io, &mut data, flags).await?;
                let mut out = MsgFlags(0);
                if full > len {
                    out |= MsgFlags::MSG_TRUNC;
                }
                Got {
                    len,
                    result: if flags.contains(MsgFlags::MSG_TRUNC) {
                        full
                    } else {
                        len
                    },
                    from: Some(netlink::kernel_addr()),
                    flags: out,
                    controls: Vec::new(),
                    rights: Vec::new(),
                }
            }
        };
        scatter(caller, &iovecs, &data[..got.len]).await?;

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
        let n = fit.min(rights.len());
        if n < rights.len() {
            *flags |= MsgFlags::MSG_CTRUNC;
        }
        let fds: Vec<Fd> = {
            let table = self.files(caller.task());
            let mut table = table.lock().unwrap();
            rights
                .into_iter()
                .take(n)
                .map(|file| table.install(file, cloexec))
                .collect()
        };
        let len = CMSG_HDR + 4 * n;
        let mut raw = Vec::with_capacity(len);
        raw.extend((len as u32).to_le_bytes());
        raw.extend(SocketLevel::SOL_SOCKET.0.to_le_bytes());
        raw.extend(unix::SCM_RIGHTS.to_le_bytes());
        for fd in fds {
            raw.extend(fd.0.to_le_bytes());
        }
        caller
            .memory()
            .write(hdr.msg_control.addr() + used as u32, &raw)
            .await?;
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
            let at = mmsg.addr() + sent * wasixv2_abi::Mmsghdr::SIZE as u32;
            let result = async {
                let hdr: UserMsghdr = read_value(caller.memory(), GuestPtr::new(at)).await?;
                let n = self.send_msghdr(caller, &sock, &hdr, flags).await?;
                let len_at = GuestPtrMut::new(at + UserMsghdr::SIZE as u32);
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
            let at = mmsg.addr() + received * wasixv2_abi::Mmsghdr::SIZE as u32;
            let result = async {
                let n = self.recv_msghdr(caller, &sock, at, flags).await?;
                let len_at = GuestPtrMut::new(at + UserMsghdr::SIZE as u32);
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
            AnySock::Netlink(..) => {
                unix::shutdown_mode(how.0)?;
                return Ok(0);
            }
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

    /// `__sys_setsockopt`: the socket layer's own options, the rest to the protocol.
    pub(crate) async fn sys_setsockopt(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        level: SocketLevel,
        name: i32,
        value: UserSlice,
    ) -> SysResult<i32> {
        let value = value.copy_in(caller.memory(), 4096).await?;
        let sock = match self.any_socket(caller, fd)? {
            AnySock::Inet((_, sock)) => sock,
            AnySock::Unix(_, sock) => {
                if level != SocketLevel::SOL_SOCKET {
                    return Err(Errno::EOPNOTSUPP.into());
                }
                if name == SocketOption::SO_PASSCRED.0 {
                    let on = value.get(..4).ok_or(Errno::EINVAL)?;
                    sock.set_passcred(i32::from_le_bytes(on.try_into().unwrap()) != 0);
                }
                // Buffer sizes, timeouts and the rest are accepted and not enforced.
                return Ok(0);
            }
            // Netlink options (buffer sizes, groups, strict checking) are accepted as no-ops.
            AnySock::Netlink(..) => return Ok(0),
        };
        if level == SocketLevel::SOL_SOCKET {
            match SocketOption(name) {
                SocketOption::SO_RCVTIMEO_OLD | SocketOption::SO_RCVTIMEO_NEW => {
                    sock.timeouts.lock().unwrap()[RCV] = parse_timeout(name, &value)?;
                    return Ok(0);
                }
                SocketOption::SO_SNDTIMEO_OLD | SocketOption::SO_SNDTIMEO_NEW => {
                    sock.timeouts.lock().unwrap()[SND] = parse_timeout(name, &value)?;
                    return Ok(0);
                }
                SocketOption::SO_TYPE
                | SocketOption::SO_PROTOCOL
                | SocketOption::SO_DOMAIN
                | SocketOption::SO_ERROR
                | SocketOption::SO_ACCEPTCONN => return Err(Errno::ENOPROTOOPT.into()),
                _ => {}
            }
        }
        sock.socket.set_option(level.0, name, &value)?;
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
        let len = usize::try_from(len).map_err(|_| Errno::EINVAL)?.min(4096);
        let mut out = vec![0; len];
        let fixed = match &any {
            AnySock::Inet(_) => None,
            AnySock::Unix(_, sock) if level == SocketLevel::SOL_SOCKET => {
                Some(self.unix_getsockopt(sock, name)?)
            }
            AnySock::Netlink(_, nl) if level == SocketLevel::SOL_SOCKET => {
                Some(nl.getsockopt(name)?)
            }
            _ => return Err(Errno::EOPNOTSUPP.into()),
        };
        if let Some(value) = fixed {
            let n = value.len().min(len);
            caller.memory().write(optval.addr(), &value[..n]).await?;
            write_value(caller.memory(), optlen, &(n as u32)).await?;
            return Ok(0);
        }
        let AnySock::Inet((_, sock)) = any else {
            unreachable!("only IP sockets get here")
        };
        let n = match self.socket_option(&sock, level, name)? {
            Some(value) => {
                let n = value.len().min(len);
                out[..n].copy_from_slice(&value[..n]);
                n
            }
            None => sock.socket.get_option(level.0, name, &mut out)?,
        };
        caller.memory().write(optval.addr(), &out[..n]).await?;
        write_value(caller.memory(), optlen, &(n as u32)).await?;
        Ok(0)
    }

    /// The `SOL_SOCKET` options the socket layer answers itself.
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
            }),
            SocketOption::SO_PROTOCOL => int(socket.protocol()),
            SocketOption::SO_DOMAIN => int(match socket.domain() {
                Domain::Inet => AddressFamily::AF_INET.0,
                Domain::Inet6 => AddressFamily::AF_INET6.0,
            }),
            SocketOption::SO_ERROR => int(socket.take_error().map_or(0, |errno| errno.0)),
            option @ (SocketOption::SO_RCVTIMEO_OLD
            | SocketOption::SO_RCVTIMEO_NEW
            | SocketOption::SO_SNDTIMEO_OLD
            | SocketOption::SO_SNDTIMEO_NEW) => {
                let dir = match option {
                    SocketOption::SO_RCVTIMEO_OLD | SocketOption::SO_RCVTIMEO_NEW => RCV,
                    _ => SND,
                };
                let timeout = sock.timeouts.lock().unwrap()[dir].unwrap_or_default();
                let (sec, usec) = (timeout.as_secs() as i64, timeout.subsec_micros() as i64);
                let mut raw;
                if option == SocketOption::SO_RCVTIMEO_OLD
                    || option == SocketOption::SO_SNDTIMEO_OLD
                {
                    let tv = KernelOldTimeval {
                        tv_sec: sec as i32,
                        tv_usec: usec as i32,
                    };
                    raw = vec![0; KernelOldTimeval::SIZE];
                    tv.write_to(&mut raw);
                } else {
                    let tv = KernelSockTimeval {
                        tv_sec: sec,
                        tv_usec: usec,
                    };
                    raw = vec![0; KernelSockTimeval::SIZE];
                    tv.write_to(&mut raw);
                }
                Ok(Some(raw))
            }
            _ => Ok(None),
        }
    }

    /// The call context for an operation on `file`.
    pub(crate) fn io<'a>(
        interrupt: &'a dyn wasixv2_wait::Interrupt,
        file: &OpenFile,
    ) -> crate::file::Io<'a> {
        crate::file::Io {
            flags: file.flags(),
            interrupt,
        }
    }
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
        false => msg.len,
    }
}

/// `MSG_DONTWAIT` makes one call non-blocking.
fn dont_wait(io: crate::file::Io<'_>, flags: MsgFlags) -> crate::file::Io<'_> {
    let mut io = io;
    if flags.contains(MsgFlags::MSG_DONTWAIT) {
        io.flags |= OpenFlags::O_NONBLOCK;
    }
    io
}

/// What a receive returns: the datagram's full length with `MSG_TRUNC`.
fn recv_result(sock: &SocketFile, flags: MsgFlags, msg: &wasixv2_vnet::RecvMsg) -> usize {
    let datagram = sock.socket.kind() == SocketKind::Datagram;
    match datagram && flags.contains(MsgFlags::MSG_TRUNC) {
        true => msg.full_len,
        false => msg.len,
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

/// The control messages of a send, each as it is; the protocol interprets them.
async fn read_cmsgs(caller: &Caller<'_>, hdr: &UserMsghdr) -> SysResult<Vec<Control>> {
    let len = hdr.msg_controllen as usize;
    if hdr.msg_control.is_null() || len == 0 {
        return Ok(Vec::new());
    }
    if len > i32::MAX as usize {
        return Err(Errno::ENOBUFS.into());
    }
    let mut raw = vec![0; len.min(64 * 1024)];
    caller
        .memory()
        .read(hdr.msg_control.addr(), &mut raw)
        .await?;
    let mut controls = Vec::new();
    let mut at = 0;
    while raw.len() - at >= CMSG_HDR {
        let word = |i: usize| u32::from_le_bytes(raw[at + i..at + i + 4].try_into().unwrap());
        let cmsg_len = word(0) as usize;
        if cmsg_len < CMSG_HDR || cmsg_len > raw.len() - at {
            return Err(Errno::EINVAL.into());
        }
        let (level, kind) = (word(4) as i32, word(8) as i32);
        let data = raw[at + CMSG_HDR..at + cmsg_len].to_vec();
        controls.push(Control { level, kind, data });
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
        caller
            .memory()
            .write(hdr.msg_control.addr() + used as u32, &raw)
            .await?;
        used += full.next_multiple_of(CMSG_ALIGN).min(left);
    }
    Ok(used)
}

/// Copies `data` out across the iovecs in order.
async fn scatter(caller: &Caller<'_>, iovecs: &[Iovec], data: &[u8]) -> SysResult<()> {
    let mut rest = data;
    for iov in iovecs {
        if rest.is_empty() {
            break;
        }
        let (chunk, tail) = rest.split_at(rest.len().min(iov.iov_len as usize));
        UserSliceMut::new(iov.iov_base, iov.iov_len)
            .copy_out(caller.memory(), chunk)
            .await?;
        rest = tail;
    }
    Ok(())
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

/// The interface requests every socket answers (`dev_ioctl`).
pub(crate) fn is_dev_ioctl(request: IoctlRequest) -> bool {
    matches!(
        request,
        IoctlRequest::SIOCGIFINDEX
            | IoctlRequest::SIOCGIFNAME
            | IoctlRequest::SIOCGIFFLAGS
            | IoctlRequest::SIOCGIFMTU
            | IoctlRequest::SIOCGIFADDR
            | IoctlRequest::SIOCGIFNETMASK
    )
}

/// `sizeof(struct ifreq)` and `IFNAMSIZ`.
const IFREQ: usize = 32;
const IFNAMSIZ: usize = 16;

impl Kernel {
    pub(crate) fn is_socket(&self, file: &OpenFile) -> bool {
        file.downcast::<SocketFile>().is_some()
            || file.downcast::<UnixFile>().is_some()
            || file.object().cloned().and_then(netlink::downcast).is_some()
    }

    /// `dev_ioctl`: `SIOCGIF*` on the namespace's interface table.
    pub(crate) async fn sys_dev_ioctl(
        &self,
        caller: &Caller<'_>,
        request: IoctlRequest,
        arg: u32,
    ) -> SysResult<i32> {
        let memory = caller.memory();
        let mut ifr = [0u8; IFREQ];
        memory.read(arg, &mut ifr).await?;
        let interfaces = self
            .network
            .as_ref()
            .map(|net| net.interfaces())
            .unwrap_or_default();
        let iface = if request == IoctlRequest::SIOCGIFNAME {
            let index = i32::from_le_bytes(ifr[IFNAMSIZ..IFNAMSIZ + 4].try_into().unwrap());
            interfaces.iter().find(|i| i.index as i32 == index)
        } else {
            let end = ifr[..IFNAMSIZ]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(IFNAMSIZ);
            let name = &ifr[..end];
            interfaces.iter().find(|i| i.name.as_bytes() == name)
        };
        let iface = iface.ok_or(Errno::ENODEV)?;
        let v4 = || {
            iface.addresses.iter().find_map(|&(ip, len)| match ip {
                std::net::IpAddr::V4(v4) => Some((v4, len)),
                _ => None,
            })
        };
        let sockaddr = |ip: std::net::Ipv4Addr| socket::encode(SocketAddr::new(ip.into(), 0));
        let value = match request {
            IoctlRequest::SIOCGIFNAME => {
                let mut name = [0u8; IFNAMSIZ];
                let n = iface.name.len().min(IFNAMSIZ - 1);
                name[..n].copy_from_slice(&iface.name.as_bytes()[..n]);
                memory.write(arg, &name).await?;
                return Ok(0);
            }
            IoctlRequest::SIOCGIFINDEX => (iface.index as i32).to_le_bytes().to_vec(),
            IoctlRequest::SIOCGIFFLAGS => (iface.flags as u16).to_le_bytes().to_vec(),
            IoctlRequest::SIOCGIFMTU => (iface.mtu as i32).to_le_bytes().to_vec(),
            IoctlRequest::SIOCGIFADDR => sockaddr(v4().ok_or(Errno::EADDRNOTAVAIL)?.0),
            _ => {
                let (_, len) = v4().ok_or(Errno::EADDRNOTAVAIL)?;
                let mask = u32::MAX.checked_shl(32 - len as u32).unwrap_or(0);
                sockaddr(mask.into())
            }
        };
        memory.write(arg + IFNAMSIZ as u32, &value).await?;
        Ok(0)
    }
}
