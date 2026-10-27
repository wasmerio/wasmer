//! Sockets: `sock_*` over Linux's socket calls.
//!
//! v1 passes addresses as tagged [`AddrPort`]s; the shims build the Linux `sockaddr` in
//! scratch and decode the kernel's. Data moves through the guest's own iovec arrays, which
//! have Linux's layout. Socket options live in [`super::sockopt`].

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use wasixv2_abi::{
    AddressFamily, ConstIovecArray, ConstSockaddrPtr, Fd, GuestPtr, GuestPtrMut, Loff, MsgFlags,
    PollEvents, Pollfd, SeekWhence, ShutdownHow, SocketFlags, SocketLevel, SocketOption,
    SocketType, UserMsghdr, VoidPtr,
};
use wasixv2_syscalls::{Control, SysErr, SysResult, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno,
        fs::Fdflags,
        sock::{
            AddrPort, Addressfamily, RiFlags, RoFlags, Sdflags, SiFlags, SockAddr, SockStatus,
            Socktype,
        },
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

use super::sockopt::{self, Timeouts};

/// Per-process network compat state: what v1 keeps about sockets and epoll sets that Linux
/// has no place for.
#[derive(Debug, Default)]
pub struct State {
    pub(crate) timeouts: Timeouts,
    pub(crate) epoll: super::epoll::Table,
}

impl State {
    /// A fork or vfork child's copy: it has the same descriptors, so the same sockets and epoll
    /// sets, under the same numbers the tables are keyed by.
    pub(crate) fn fork(&self) -> Self {
        Self {
            timeouts: self.timeouts.fork(),
            epoll: self.epoll.fork(),
        }
    }
}

/// Room for any `sockaddr` the kernel returns (`sockaddr_un` is the largest, 110 bytes).
const SOCKADDR_ROOM: u32 = 128;

/// The Linux address family for a v1 one.
pub(crate) fn family(af: Addressfamily) -> V1Result<AddressFamily> {
    Ok(match af {
        Addressfamily::INET4 => AddressFamily::AF_INET,
        Addressfamily::INET6 => AddressFamily::AF_INET6,
        Addressfamily::UNIX => AddressFamily::AF_UNIX,
        _ => return shim::fail(Errno::AFNOSUPPORT),
    })
}

/// The Linux `socket` type argument for wasix-libc's: the v1 type in the low byte, with
/// libc's `SOCK_NONBLOCK`/`SOCK_CLOEXEC` bits translated.
fn socket_type(ty: i32) -> V1Result<i32> {
    let ty = ty as u32;
    let flags = ty & !0xff;
    if flags & !(Socktype::LIBC_NONBLOCK | Socktype::LIBC_CLOEXEC) != 0 {
        return shim::fail(Errno::INVAL);
    }
    let kind = match Socktype(ty as u8) {
        Socktype::STREAM => SocketType::SOCK_STREAM,
        Socktype::DGRAM => SocketType::SOCK_DGRAM,
        Socktype::RAW => SocketType::SOCK_RAW,
        Socktype::SEQPACKET => SocketType::SOCK_SEQPACKET,
        _ => return shim::fail(Errno::INVAL),
    };
    let mut linux = kind.0;
    if flags & Socktype::LIBC_NONBLOCK != 0 {
        linux |= SocketFlags::SOCK_NONBLOCK.0 as i32;
    }
    if flags & Socktype::LIBC_CLOEXEC != 0 {
        linux |= SocketFlags::SOCK_CLOEXEC.0 as i32;
    }
    Ok(linux)
}

/// The protocol argument: v1's numbers are IP protocol numbers. wasix-libc fills in TCP or
/// UDP for a zero protocol even for unix sockets, which take none.
fn protocol(family: AddressFamily, proto: i32) -> i32 {
    if family == AddressFamily::AF_UNIX {
        0
    } else {
        proto
    }
}

/// `addr` as a Linux `sockaddr`.
pub(crate) fn to_linux(addr: &SockAddr) -> V1Result<Vec<u8>> {
    let family = |af: AddressFamily| (af.0 as u16).to_le_bytes();
    Ok(match addr {
        SockAddr::Inet(SocketAddr::V4(v4)) => {
            let mut out = Vec::with_capacity(16);
            out.extend_from_slice(&family(AddressFamily::AF_INET));
            out.extend_from_slice(&v4.port().to_be_bytes());
            out.extend_from_slice(&v4.ip().octets());
            out.resize(16, 0);
            out
        }
        SockAddr::Inet(SocketAddr::V6(v6)) => {
            let mut out = Vec::with_capacity(28);
            out.extend_from_slice(&family(AddressFamily::AF_INET6));
            out.extend_from_slice(&v6.port().to_be_bytes());
            out.extend_from_slice(&v6.flowinfo().to_le_bytes());
            out.extend_from_slice(&v6.ip().octets());
            out.extend_from_slice(&v6.scope_id().to_le_bytes());
            out
        }
        SockAddr::Unix(path) => {
            let mut out = Vec::with_capacity(path.len() + 3);
            out.extend_from_slice(&family(AddressFamily::AF_UNIX));
            out.extend_from_slice(path);
            // An empty path asks for autobind and has no NUL.
            if !path.is_empty() {
                out.push(0);
            }
            out
        }
        SockAddr::Unspec => return shim::fail(Errno::AFNOSUPPORT),
    })
}

/// The address in a Linux `sockaddr` of `bytes.len()` bytes; [`SockAddr::Unspec`] for an
/// empty or unknown one.
pub(crate) fn from_linux(bytes: &[u8]) -> SockAddr {
    let Some(family) = bytes.get(..2) else {
        return SockAddr::Unspec;
    };
    let family = AddressFamily(i32::from(u16::from_le_bytes([family[0], family[1]])));
    let port = |b: &[u8]| u16::from_be_bytes([b[2], b[3]]);
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    match family {
        AddressFamily::AF_INET if bytes.len() >= 8 => {
            let ip: [u8; 4] = bytes[4..8].try_into().unwrap();
            SockAddr::Inet(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(ip),
                port(bytes),
            )))
        }
        AddressFamily::AF_INET6 if bytes.len() >= 28 => {
            let ip: [u8; 16] = bytes[8..24].try_into().unwrap();
            SockAddr::Inet(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(ip),
                port(bytes),
                u32_at(bytes, 4),
                u32_at(bytes, 24),
            )))
        }
        AddressFamily::AF_UNIX => {
            let path = &bytes[2..];
            let len = path.iter().position(|&b| b == 0).unwrap_or(path.len());
            SockAddr::Unix(path[..len].to_vec())
        }
        _ => SockAddr::Unspec,
    }
}

/// The guest's [`AddrPort`] at `ptr` as a Linux `sockaddr` in scratch.
async fn sockaddr_in<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    ptr: i32,
) -> V1Result<(ConstSockaddrPtr, u32)> {
    let addr: AddrPort = frame.read(ptr as u32).await?;
    let Some(addr) = addr.decode() else {
        return shim::fail(Errno::INVAL);
    };
    let bytes = to_linux(&addr)?;
    let slice = frame.mem.alloc_bytes(&bytes)?;
    Ok((GuestPtr::new(slice.addr), slice.len))
}

/// Scratch room for a `sockaddr` the kernel writes, and its length in and out.
pub(crate) struct SockaddrOut {
    pub(crate) addr: GuestPtrMut<wasixv2_abi::Sockaddr>,
    pub(crate) len: GuestPtrMut<u32>,
}

impl SockaddrOut {
    pub(crate) fn new<S: 'static>(frame: &Frame<'_, S>) -> SysResult<Self> {
        Ok(Self {
            addr: GuestPtrMut::new(frame.mem.alloc_zeroed(SOCKADDR_ROOM)?.addr),
            len: GuestPtrMut::new(frame.mem.alloc(&SOCKADDR_ROOM)?.addr()),
        })
    }

    /// What the kernel left.
    pub(crate) fn take<S: 'static>(&self, frame: &Frame<'_, S>) -> SockAddr {
        let len = frame.mem.take(self.len).min(SOCKADDR_ROOM);
        let bytes = frame.mem.take_bytes(
            wasixv2_syscalls::UserSliceMut::new(self.addr.addr(), SOCKADDR_ROOM),
            len as usize,
        );
        from_linux(&bytes)
    }
}

pub(crate) async fn sock_open<S: Syscalls + Control + 'static>(
    env: Env<S>,
    af: i32,
    ty: i32,
    proto: i32,
    ro_fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let family = family(Addressfamily(af as u8))?;
        let ty = socket_type(ty)?;
        let fd = frame
            .syscalls
            .socket(&frame.caller(), family, ty, protocol(family, proto))
            .await?;
        sockopt::new_socket(frame, fd).await?;
        frame.write(ro_fd as u32, &(fd.0 as u32)).await
    })
    .await
}

pub(crate) async fn sock_pair<S: Syscalls + Control + 'static>(
    env: Env<S>,
    af: i32,
    ty: i32,
    proto: i32,
    ro_fd1: i32,
    ro_fd2: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let family = family(Addressfamily(af as u8))?;
        let ty = socket_type(ty)?;
        let fds = frame.mem.alloc_mut::<[i32; 2]>()?;
        frame
            .syscalls
            .socketpair(
                &frame.caller(),
                family,
                ty,
                protocol(family, proto),
                GuestPtrMut::new(fds.addr()),
            )
            .await?;
        let [a, b] = frame.mem.take(fds);
        sockopt::new_socket(frame, Fd(a)).await?;
        sockopt::new_socket(frame, Fd(b)).await?;
        frame.write(ro_fd1 as u32, &(a as u32)).await?;
        frame.write(ro_fd2 as u32, &(b as u32)).await
    })
    .await
}

pub(crate) async fn sock_bind<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    addr: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (ptr, len) = sockaddr_in(frame, addr).await?;
        frame
            .syscalls
            .bind(&frame.caller(), Fd(fd), ptr, len)
            .await?;
        Ok(())
    })
    .await
}

pub(crate) async fn sock_listen<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    backlog: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame
            .syscalls
            .listen(&frame.caller(), Fd(fd), backlog)
            .await?;
        Ok(())
    })
    .await
}

/// `connect`; a `CONNECT_TIMEOUT` the guest set bounds a blocking connect.
pub(crate) async fn sock_connect<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    addr: i32,
) -> Result<i32, RuntimeError> {
    // Whether the body runs again after a signal interrupted it.
    let mut again = false;
    shim::errno(&env, async |frame| {
        let (ptr, len) = sockaddr_in(frame, addr).await?;
        match sockopt::connect_timeout(frame, Fd(fd)).await? {
            Some(timeout) => {
                let result = sockopt::connect_within(frame, Fd(fd), ptr, len, timeout, again);
                again = true;
                result.await
            }
            None => {
                frame
                    .syscalls
                    .connect(&frame.caller(), Fd(fd), ptr, len)
                    .await?;
                Ok(())
            }
        }
    })
    .await
}

/// `accept4`, after waiting out an `ACCEPT_TIMEOUT` the guest set. Returns the new
/// descriptor and the peer.
async fn accept<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: i32,
    fdflags: i32,
) -> V1Result<(Fd, SockAddr)> {
    let fdflags = Fdflags(fdflags as u16);
    let mut flags = SocketFlags::empty();
    if fdflags.0 & Fdflags::NONBLOCK.0 != 0 {
        flags |= SocketFlags::SOCK_NONBLOCK;
    }
    sockopt::wait_accept(frame, Fd(fd)).await?;
    let peer = SockaddrOut::new(frame)?;
    let new = frame
        .syscalls
        .accept4(&frame.caller(), Fd(fd), peer.addr, peer.len, flags)
        .await?;
    sockopt::new_socket(frame, new).await?;
    Ok((new, peer.take(frame)))
}

/// preview1's `sock_accept(fd, flags, ro_fd)`, without the peer address.
pub(crate) async fn sock_accept_preview1<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    fdflags: i32,
    ro_fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (new, _) = accept(frame, fd, fdflags).await?;
        frame.write(ro_fd as u32, &(new.0 as u32)).await
    })
    .await
}

/// `sock_accept_v2(fd, flags, ro_fd, ro_addr)`, also `wasix_32v1`'s `sock_accept`.
pub(crate) async fn sock_accept_v2<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    fdflags: i32,
    ro_fd: i32,
    ro_addr: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (new, peer) = accept(frame, fd, fdflags).await?;
        frame.write(ro_fd as u32, &(new.0 as u32)).await?;
        frame.write(ro_addr as u32, &AddrPort::encode(&peer)).await
    })
    .await
}

/// The Linux flags for v1 receive flags.
fn recv_flags(ri: RiFlags) -> MsgFlags {
    let mut flags = MsgFlags(0);
    for (v1, linux) in [
        (RiFlags::RECV_PEEK, MsgFlags::MSG_PEEK),
        (RiFlags::RECV_WAITALL, MsgFlags::MSG_WAITALL),
        (RiFlags::RECV_DATA_TRUNCATED, MsgFlags::MSG_TRUNC),
        (RiFlags::RECV_DONT_WAIT, MsgFlags::MSG_DONTWAIT),
    ] {
        if ri.0 & v1.0 != 0 {
            flags.0 |= linux.0;
        }
    }
    flags
}

/// The Linux flags for v1 send flags. v1 never raised `SIGPIPE` for sockets.
fn send_flags(si: SiFlags) -> MsgFlags {
    let mut flags = MsgFlags::MSG_NOSIGNAL;
    if si.0 & SiFlags::SEND_DONT_WAIT.0 != 0 {
        flags.0 |= MsgFlags::MSG_DONTWAIT.0;
    }
    flags
}

/// A `msghdr` in scratch over the guest's iovec array, with room for the peer address when
/// `peer` is given.
fn msghdr<S: 'static>(
    frame: &Frame<'_, S>,
    iovs: i32,
    iovs_len: i32,
    name: Option<(u32, u32)>,
) -> SysResult<GuestPtrMut<UserMsghdr>> {
    let (name, namelen) = name.unwrap_or((0, 0));
    let msg = UserMsghdr {
        msg_name: GuestPtrMut::new(name),
        msg_namelen: namelen as i32,
        msg_iov: ConstIovecArray::new(iovs as u32),
        msg_iovlen: iovs_len as u32,
        msg_control: VoidPtr::NULL,
        msg_controllen: 0,
        msg_flags: 0,
    };
    Ok(GuestPtrMut::new(frame.mem.alloc(&msg)?.addr()))
}

/// `recvmsg` into the guest's iovecs: bytes received, v1 output flags and, when `want_peer`,
/// the sender.
async fn recv<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    ri_flags: i32,
    want_peer: bool,
) -> V1Result<(u32, RoFlags, SockAddr)> {
    let name = if want_peer {
        Some((frame.mem.alloc_zeroed(SOCKADDR_ROOM)?.addr, SOCKADDR_ROOM))
    } else {
        None
    };
    let msg = msghdr(frame, iovs, iovs_len, name)?;
    let received = frame
        .syscalls
        .recvmsg(
            &frame.caller(),
            Fd(fd),
            msg,
            recv_flags(RiFlags(ri_flags as u16)),
        )
        .await;
    let n = match received {
        // v1 let `sock_recv` read any stream, such as a pipe.
        Err(SysErr::Errno(wasixv2_abi::Errno::ENOTSOCK)) if !want_peer && ri_flags == 0 => {
            let n = frame
                .syscalls
                .readv(
                    &frame.caller(),
                    Fd(fd),
                    ConstIovecArray::new(iovs as u32),
                    iovs_len as u32,
                )
                .await?;
            return Ok((n, RoFlags(0), SockAddr::Unspec));
        }
        other => other?,
    };
    let out = frame.mem.take(msg);
    let mut ro = RoFlags(0);
    if out.msg_flags & MsgFlags::MSG_TRUNC.0 != 0 {
        ro.0 |= RoFlags::RECV_DATA_TRUNCATED.0;
    }
    let mut peer = SockAddr::Unspec;
    if let Some((addr, _)) = name {
        let len = (out.msg_namelen.max(0) as u32).min(SOCKADDR_ROOM);
        let bytes = frame.mem.take_bytes(
            wasixv2_syscalls::UserSliceMut::new(addr, SOCKADDR_ROOM),
            len as usize,
        );
        peer = from_linux(&bytes);
        if peer == SockAddr::Unspec {
            // A connected stream names no sender; v1 reported the peer.
            peer = peer_name(frame, Fd(fd)).await.unwrap_or(SockAddr::Unspec);
        }
    }
    Ok((n as u32, ro, peer))
}

/// `getpeername`, best effort.
async fn peer_name<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<SockAddr> {
    let out = SockaddrOut::new(frame)?;
    frame
        .syscalls
        .getpeername(&frame.caller(), fd, out.addr, out.len)
        .await?;
    Ok(out.take(frame))
}

pub(crate) async fn sock_recv<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    ri_flags: i32,
    ro_datalen: i32,
    ro_flags: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (n, flags, _) = recv(frame, fd, iovs, iovs_len, ri_flags, false).await?;
        frame.write(ro_datalen as u32, &n).await?;
        frame.write(ro_flags as u32, &flags).await
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn sock_recv_from<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    ri_flags: i32,
    ro_datalen: i32,
    ro_flags: i32,
    ro_addr: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (n, flags, peer) = recv(frame, fd, iovs, iovs_len, ri_flags, true).await?;
        frame.write(ro_datalen as u32, &n).await?;
        frame.write(ro_flags as u32, &flags).await?;
        frame.write(ro_addr as u32, &AddrPort::encode(&peer)).await
    })
    .await
}

/// `sendmsg` from the guest's iovecs, to `to` if given.
async fn send<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    si_flags: i32,
    to: Option<i32>,
) -> V1Result<u32> {
    let name = match to {
        Some(addr) => {
            let (ptr, len) = sockaddr_in(frame, addr).await?;
            Some((ptr.addr(), len))
        }
        None => None,
    };
    let msg = msghdr(frame, iovs, iovs_len, name)?;
    let sent = frame
        .syscalls
        .sendmsg(
            &frame.caller(),
            Fd(fd),
            GuestPtr::new(msg.addr()),
            send_flags(SiFlags(si_flags as u16)),
        )
        .await;
    match sent {
        // v1 let `sock_send` write any stream, such as a pipe.
        Err(SysErr::Errno(wasixv2_abi::Errno::ENOTSOCK)) if to.is_none() && si_flags == 0 => {
            Ok(frame
                .syscalls
                .writev(
                    &frame.caller(),
                    Fd(fd),
                    ConstIovecArray::new(iovs as u32),
                    iovs_len as u32,
                )
                .await?)
        }
        other => Ok(other? as u32),
    }
}

pub(crate) async fn sock_send<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    si_flags: i32,
    ro_datalen: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let n = send(frame, fd, iovs, iovs_len, si_flags, None).await?;
        frame.write(ro_datalen as u32, &n).await
    })
    .await
}

pub(crate) async fn sock_send_to<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    si_flags: i32,
    addr: i32,
    ro_datalen: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let n = send(frame, fd, iovs, iovs_len, si_flags, Some(addr)).await?;
        frame.write(ro_datalen as u32, &n).await
    })
    .await
}

pub(crate) async fn sock_shutdown<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    how: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let how = Sdflags(how as u8);
        let (rd, wr) = (how.0 & Sdflags::RD.0 != 0, how.0 & Sdflags::WR.0 != 0);
        let how = match (rd, wr) {
            (true, true) => ShutdownHow::SHUT_RDWR,
            (true, false) => ShutdownHow::SHUT_RD,
            (false, true) => ShutdownHow::SHUT_WR,
            (false, false) => return shim::fail(Errno::INVAL),
        };
        frame
            .syscalls
            .shutdown(&frame.caller(), Fd(fd), how)
            .await?;
        Ok(())
    })
    .await
}

pub(crate) async fn sock_addr_local<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    ro_addr: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let out = SockaddrOut::new(frame)?;
        frame
            .syscalls
            .getsockname(&frame.caller(), Fd(fd), out.addr, out.len)
            .await?;
        frame
            .write(ro_addr as u32, &AddrPort::encode(&out.take(frame)))
            .await
    })
    .await
}

pub(crate) async fn sock_addr_peer<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    ro_addr: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let peer = peer_name(frame, Fd(fd)).await?;
        frame.write(ro_addr as u32, &AddrPort::encode(&peer)).await
    })
    .await
}

/// `getsockopt(SOL_SOCKET, name)` as an int.
pub(crate) async fn int_option<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    level: SocketLevel,
    name: i32,
) -> V1Result<i32> {
    let value = frame.mem.alloc_mut::<i32>()?;
    let len = frame.mem.alloc(&4u32)?;
    frame
        .syscalls
        .getsockopt(
            &frame.caller(),
            fd,
            level,
            name,
            GuestPtrMut::new(value.addr()),
            GuestPtrMut::new(len.addr()),
        )
        .await?;
    Ok(frame.mem.take(value))
}

/// v1's coarse connection state, from what Linux reports without side effects: connected
/// (or listening, or a datagram socket) is `OPENED`, a pending error `FAILED`, anything else
/// still `OPENING`.
pub(crate) async fn sock_status<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    ro_status: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let fd = Fd(fd);
        let ty = int_option(frame, fd, SocketLevel::SOL_SOCKET, SocketOption::SO_TYPE.0).await?;
        let listening = int_option(
            frame,
            fd,
            SocketLevel::SOL_SOCKET,
            SocketOption::SO_ACCEPTCONN.0,
        )
        .await
        .unwrap_or(0);
        let status = if ty == SocketType::SOCK_DGRAM.0
            || listening != 0
            || peer_name(frame, fd).await.is_ok()
        {
            SockStatus::OPENED
        } else {
            let pollfd = Pollfd {
                fd,
                events: PollEvents::POLLOUT,
                revents: PollEvents(0),
            };
            let ptr = frame.mem.alloc(&pollfd)?;
            let zero = frame.mem.alloc(&wasixv2_abi::KernelTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            })?;
            frame
                .syscalls
                .ppoll_time64(
                    &frame.caller(),
                    GuestPtrMut::new(ptr.addr()),
                    1,
                    GuestPtrMut::new(zero.addr()),
                    GuestPtr::NULL,
                    8,
                )
                .await?;
            let revents = frame
                .mem
                .take(GuestPtrMut::<Pollfd>::new(ptr.addr()))
                .revents;
            if revents.0 & PollEvents::POLLERR.0 != 0 {
                SockStatus::FAILED
            } else {
                SockStatus::OPENING
            }
        };
        frame.write(ro_status as u32, &status).await
    })
    .await
}

/// `sock_send_file(out, in, offset, count, ro_sent)`: sends `count` bytes of `in` from
/// `offset`, advancing `in`'s file position past them as v1 did (wasix-libc's `sendfile`
/// relies on that when it is given no offset). Short transfers repeat until `count` bytes
/// went or `in` ends; a later error ends the call with what was sent.
pub(crate) async fn sock_send_file<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out_fd: i32,
    in_fd: i32,
    offset: i64,
    count: i64,
    ro_sent: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        match frame
            .syscalls
            .lseek(&caller, Fd(in_fd), offset as Loff, SeekWhence::SEEK_SET)
            .await
        {
            // Pipes and sockets have no position; they send from where they are.
            Err(wasixv2_syscalls::SysErr::Errno(errno)) if errno == wasixv2_abi::Errno::ESPIPE => {}
            other => {
                other?;
            }
        }
        let mut left = count as u64;
        let mut sent = 0u64;
        while left > 0 {
            let chunk = left.min(0x7fff_f000) as u32;
            let n = match frame
                .syscalls
                .sendfile64(&caller, Fd(out_fd), Fd(in_fd), GuestPtrMut::NULL, chunk)
                .await
            {
                Ok(n) => n as u64,
                Err(err) if sent == 0 => return Err(err.into()),
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            sent += n;
            left -= n;
        }
        frame.write(ro_sent as u32, &sent).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockaddr_round_trips() {
        let addrs = [
            SockAddr::Inet("10.1.2.3:53".parse().unwrap()),
            SockAddr::Inet(SocketAddr::V6(SocketAddrV6::new(
                "2001:db8::1".parse().unwrap(),
                8443,
                7,
                3,
            ))),
            SockAddr::Unix(b"/run/x.sock".to_vec()),
        ];
        for addr in addrs {
            assert_eq!(from_linux(&to_linux(&addr).unwrap()), addr);
        }
    }

    #[test]
    fn sockaddr_in_bytes() {
        let bytes = to_linux(&SockAddr::Inet("127.0.0.1:9000".parse().unwrap())).unwrap();
        assert_eq!(
            bytes,
            [2, 0, 0x23, 0x28, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(to_linux(&SockAddr::Unix(Vec::new())).unwrap(), [1, 0]);
        assert_eq!(from_linux(&[]), SockAddr::Unspec);
    }

    #[test]
    fn socket_types() {
        assert_eq!(socket_type(1).unwrap(), SocketType::SOCK_STREAM.0);
        assert_eq!(socket_type(4).unwrap(), SocketType::SOCK_SEQPACKET.0);
        assert_eq!(
            socket_type(2 | 0x4000 | 0x2000).unwrap(),
            SocketType::SOCK_DGRAM.0
                | SocketFlags::SOCK_NONBLOCK.0 as i32
                | SocketFlags::SOCK_CLOEXEC.0 as i32
        );
        assert!(socket_type(0).is_err());
        assert!(socket_type(1 | 0x100).is_err());
    }

    #[test]
    fn message_flags() {
        assert_eq!(
            recv_flags(RiFlags(0xf)).0,
            MsgFlags::MSG_PEEK.0
                | MsgFlags::MSG_WAITALL.0
                | MsgFlags::MSG_TRUNC.0
                | MsgFlags::MSG_DONTWAIT.0
        );
        assert_eq!(
            send_flags(SiFlags::SEND_DONT_WAIT).0,
            MsgFlags::MSG_NOSIGNAL.0 | MsgFlags::MSG_DONTWAIT.0
        );
    }
}
