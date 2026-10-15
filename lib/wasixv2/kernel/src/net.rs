//! The socket syscalls, as Linux's `net/socket.c`: descriptor handling, guest struct decoding
//! (`move_addr_to_kernel`, `move_addr_to_user`, msghdr and cmsg layouts), blocking and the
//! `SOL_SOCKET` options the socket layer itself answers. IP protocol behaviour comes from the
//! network namespace (`wasixv2_vnet::Network`); Unix sockets are not implemented yet.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::{
    AddressFamily, Buf, ConstSockaddrPtr, ConstUserMsghdrPtr, FdPair, GuestLayout, GuestPtr,
    GuestPtrMut, Iovec, KernelOldTimeval, KernelSockTimeval, MmsghdrArray, MsgFlags, OpenFlags,
    ShutdownHow, SockaddrPtr, SocketFlags, SocketLevel, SocketOption, SocketType, Socklen,
    SocklenPtr, UserMsghdr, UserMsghdrPtr,
};
use wasixv2_syscalls::{
    Caller, Errno, Fd, SysErr, SysResult, UserSlice, UserSliceMut, read_value, write_value,
};
use wasixv2_vnet::{Control, Domain, SendMsg, Shutdown, SocketKind};

use crate::{
    IO_CHUNK, Kernel,
    file::{Backing, OpenFile},
    fs::read_iovecs,
    socket::{self, RCV, SND, SOCKADDR_STORAGE, SocketFile},
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

impl Kernel {
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
            // TODO: AF_UNIX, a kernel facility of its own. Every other family (and anything
            // past NPROTO) has no implementation.
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
        _caller: &Caller<'_>,
        family: AddressFamily,
        r#type: i32,
        _protocol: i32,
        _usockvec: FdPair,
    ) -> SysResult<i32> {
        socket_type(r#type)?;
        match family {
            // TODO: AF_UNIX socket pairs.
            AddressFamily::AF_UNIX => Err(Errno::EAFNOSUPPORT.into()),
            AddressFamily::AF_INET | AddressFamily::AF_INET6 => Err(Errno::EOPNOTSUPP.into()),
            _ => Err(Errno::EAFNOSUPPORT.into()),
        }
    }

    /// `__sys_bind` and `inet_bind`.
    pub(crate) async fn sys_bind(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        addr: ConstSockaddrPtr,
        len: Socklen,
    ) -> SysResult<i32> {
        let (_, sock) = self.socket_fd(caller, fd)?;
        let raw = move_addr_to_kernel(caller, addr, len).await?;
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
        let (_, sock) = self.socket_fd(caller, fd)?;
        sock.socket.listen((backlog as u32).min(SOMAXCONN))?;
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let io = self.io(caller, &file);
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let raw = move_addr_to_kernel(caller, addr, len).await?;
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
        let io = self.io(caller, &file);
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
        let (_, sock) = self.socket_fd(caller, fd)?;
        let raw = socket::encode(sock.socket.local_addr()?);
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
        let (_, sock) = self.socket_fd(caller, fd)?;
        let raw = socket::encode(sock.socket.peer_addr()?);
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
        let (file, sock) = self.socket_fd(caller, fd)?;
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
        Ok(sock.send(self.io(caller, &file), msg).await? as u32)
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let msg = sock.recv(self.io(caller, &file), &mut data, flags).await?;
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let hdr = read_value(caller.memory(), msg).await?;
        Ok(self.send_msghdr(caller, &file, &sock, &hdr, flags).await? as i32)
    }

    async fn send_msghdr(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &SocketFile,
        hdr: &UserMsghdr,
        flags: MsgFlags,
    ) -> SysResult<usize> {
        let memory = caller.memory();
        let namelen = msg_namelen(hdr)?;
        if hdr.msg_iovlen > UIO_MAXIOV {
            return Err(Errno::EMSGSIZE.into());
        }
        let to = match hdr.msg_name.is_null() || namelen == 0 {
            true => None,
            false => {
                let raw = move_addr_to_kernel(caller, GuestPtr::new(hdr.msg_name.addr()), namelen)
                    .await?;
                self.destination(sock, &raw)?
            }
        };
        let control = read_cmsgs(caller, hdr).await?;
        let mut data = Vec::new();
        for iov in read_iovecs(memory, GuestPtr::new(hdr.msg_iov.addr()), hdr.msg_iovlen).await? {
            let slice = UserSlice::new(iov.iov_base, iov.iov_len);
            data.extend(slice.copy_in(memory, IO_CHUNK - data.len()).await?);
        }
        let msg = SendMsg {
            data: &data,
            to,
            flags,
            control: &control,
        };
        sock.send(self.io(caller, file), msg).await
    }

    /// `___sys_recvmsg`.
    pub(crate) async fn sys_recvmsg(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        msg: UserMsghdrPtr,
        flags: MsgFlags,
    ) -> SysResult<i32> {
        let (file, sock) = self.socket_fd(caller, fd)?;
        Ok(self
            .recv_msghdr(caller, &file, &sock, msg.addr(), flags)
            .await? as i32)
    }

    /// Receives into the msghdr at `at` and writes back its name, control and flags.
    async fn recv_msghdr(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &SocketFile,
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
        let msg = sock.recv(self.io(caller, file), &mut data, flags).await?;
        scatter(caller, &iovecs, &data[..msg.len]).await?;

        // The name: as much as fits, with the real length reported.
        hdr.msg_namelen = 0;
        if let (false, Some(from)) = (hdr.msg_name.is_null(), msg.from) {
            let raw = socket::encode(from);
            let n = raw.len().min(namelen as usize);
            memory.write(hdr.msg_name.addr(), &raw[..n]).await?;
            hdr.msg_namelen = raw.len() as i32;
        }
        let mut out_flags = msg.flags;
        let used = write_cmsgs(caller, &hdr, &msg.control, &mut out_flags).await?;
        hdr.msg_controllen = used as u32;
        hdr.msg_flags = out_flags.0;
        write_value(memory, GuestPtrMut::new(at), &hdr).await?;
        Ok(recv_result(sock, flags, &msg))
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let vlen = vlen.min(UIO_MAXIOV);
        let mut sent = 0;
        while sent < vlen {
            let at = mmsg.addr() + sent * wasixv2_abi::Mmsghdr::SIZE as u32;
            let result = async {
                let hdr: UserMsghdr = read_value(caller.memory(), GuestPtr::new(at)).await?;
                let n = self.send_msghdr(caller, &file, &sock, &hdr, flags).await?;
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
        let (file, sock) = self.socket_fd(caller, fd)?;
        let vlen = vlen.min(UIO_MAXIOV);
        let mut flags = flags;
        let mut received = 0;
        while received < vlen {
            let at = mmsg.addr() + received * wasixv2_abi::Mmsghdr::SIZE as u32;
            let result = async {
                let n = self.recv_msghdr(caller, &file, &sock, at, flags).await?;
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
        let (_, sock) = self.socket_fd(caller, fd)?;
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
        let (_, sock) = self.socket_fd(caller, fd)?;
        let value = value.copy_in(caller.memory(), 4096).await?;
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
        let (_, sock) = self.socket_fd(caller, fd)?;
        let len: i32 = read_value(caller.memory(), GuestPtr::new(optlen.addr())).await?;
        let len = usize::try_from(len).map_err(|_| Errno::EINVAL)?.min(4096);
        let mut out = vec![0; len];
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
    fn io<'a>(&'a self, caller: &Caller<'_>, file: &OpenFile) -> crate::file::Io<'a> {
        crate::file::Io {
            flags: file.flags(),
            interrupt: self.interrupt(caller),
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

/// `__scm_send` for IP sockets: the control messages, each passed on as it is. `SCM_RIGHTS`
/// and `SCM_CREDENTIALS` only work on Unix sockets.
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
        if level == SocketLevel::SOL_SOCKET.0 && matches!(kind, 1 | 2) {
            return Err(Errno::EINVAL.into());
        }
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
