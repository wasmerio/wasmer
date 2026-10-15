//! IP sockets as open-file objects: a `wasixv2_vnet::Socket` plus what the kernel owns about
//! it (timeouts, blocking), and the guest `sockaddr` codec.

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::{Arc, Mutex},
    time::Duration,
};

use wasixv2_abi::{AddressFamily, GuestPtrMut, IoctlRequest, MsgFlags};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vnet::{Domain, RecvMsg, SendMsg, Socket, SocketKind};
use wasixv2_wait::{PollMask, PollTable, PollWaiter, Pollable, Wakeup};

use crate::file::{FileObject, Io};

/// `sizeof(struct sockaddr_storage)`, the most `move_addr_to_kernel` takes.
pub(crate) const SOCKADDR_STORAGE: u32 = 128;
const SOCKADDR_IN: usize = 16;
/// `SIN6_LEN_RFC2133`: an IPv6 address without the scope id is still accepted.
const SOCKADDR_IN6_MIN: usize = 24;
const SOCKADDR_IN6: usize = 28;

/// The kernel's side of an IP socket.
pub(crate) struct SocketFile {
    pub(crate) socket: Arc<dyn Socket>,
    host: Arc<dyn Host>,
    /// `SO_RCVTIMEO` and `SO_SNDTIMEO`; `None` waits forever.
    pub(crate) timeouts: Mutex<[Option<Duration>; 2]>,
}

pub(crate) const RCV: usize = 0;
pub(crate) const SND: usize = 1;

impl SocketFile {
    pub(crate) fn new(socket: Arc<dyn Socket>, host: Arc<dyn Host>) -> Self {
        Self {
            socket,
            host,
            timeouts: Mutex::new([None, None]),
        }
    }

    /// `sk_wait_event` around a non-blocking operation: retries `op` while it fails with
    /// `EAGAIN` until the socket reports one of `events`. A timeout ends the wait with
    /// `EAGAIN`; a signal with a restart, or `EINTR` when a timeout was set
    /// (`sock_intr_errno`).
    pub(crate) async fn block<T>(
        &self,
        io: Io<'_>,
        dir: usize,
        events: PollMask,
        mut op: impl FnMut() -> Result<T, Errno>,
    ) -> SysResult<T> {
        let timeout = self.timeouts.lock().unwrap()[dir];
        let mut waiter = PollWaiter::new();
        let mut timer = timeout.map(|timeout| self.host.sleep(timeout));
        let mut registered = false;
        let key = events | PollMask::ERR | PollMask::HUP;
        loop {
            match op() {
                Err(Errno::EAGAIN) if !io.nonblocking() => {}
                result => return Ok(result?),
            }
            if !registered {
                // On the queues before the retry, so no wakeup is lost.
                waiter.poll(&*self.socket, key, true);
                registered = true;
                continue;
            }
            let interrupted = || match timeout {
                Some(_) => SysErr::Errno(Errno::EINTR),
                None => SysErr::Restart,
            };
            if io.interrupt.pending() {
                return Err(interrupted());
            }
            let timer = timer.as_mut().map(|timer| timer.as_mut());
            match waiter.sleep(io.interrupt, timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => return Err(Errno::EAGAIN.into()),
                Wakeup::Interrupted => return Err(interrupted()),
            }
        }
    }

    /// Receives into `buf`, waiting unless non-blocking; with `MSG_WAITALL` on a stream, until
    /// `buf` is full, the stream ends or an error.
    pub(crate) async fn recv(
        &self,
        io: Io<'_>,
        buf: &mut [u8],
        flags: MsgFlags,
    ) -> SysResult<RecvMsg> {
        let io = dont_wait(io, flags);
        let socket = &self.socket;
        let proto_flags = flags & !(MsgFlags::MSG_WAITALL | MsgFlags::MSG_DONTWAIT);
        let mut msg = self
            .block(io, RCV, PollMask::READABLE, || {
                socket.recv(buf, proto_flags)
            })
            .await?;
        let waitall = flags.contains(MsgFlags::MSG_WAITALL)
            && !flags.contains(MsgFlags::MSG_PEEK)
            && socket.kind() == SocketKind::Stream;
        while waitall && msg.len > 0 && msg.len < buf.len() {
            let rest = &mut buf[msg.len..];
            match self
                .block(io, RCV, PollMask::READABLE, || {
                    socket.recv(rest, proto_flags)
                })
                .await
            {
                Ok(more) if more.len > 0 => {
                    msg.len += more.len;
                    msg.full_len += more.full_len;
                }
                _ => break,
            }
        }
        Ok(msg)
    }

    /// Sends `msg`, waiting for room unless non-blocking. A blocking stream send goes on until
    /// everything is sent, returning the partial count if it then fails.
    pub(crate) async fn send(&self, io: Io<'_>, msg: SendMsg<'_>) -> SysResult<usize> {
        let io = dont_wait(io, msg.flags);
        let socket = &self.socket;
        let flags = msg.flags & !(MsgFlags::MSG_DONTWAIT | MsgFlags::MSG_NOSIGNAL);
        let stream = socket.kind() == SocketKind::Stream;
        let mut sent = 0;
        loop {
            let part = SendMsg {
                data: &msg.data[sent..],
                flags,
                ..msg
            };
            match self
                .block(io, SND, PollMask::WRITABLE, || socket.send(part))
                .await
            {
                Ok(n) => sent += n,
                Err(_) if sent > 0 => return Ok(sent),
                // TODO: raise SIGPIPE for EPIPE without MSG_NOSIGNAL once signals exist.
                Err(err) => return Err(err),
            }
            if !stream || io.nonblocking() || sent >= msg.data.len() {
                return Ok(sent);
            }
        }
    }
}

/// `MSG_DONTWAIT` makes one call non-blocking.
fn dont_wait(io: Io<'_>, flags: MsgFlags) -> Io<'_> {
    let mut io = io;
    if flags.contains(MsgFlags::MSG_DONTWAIT) {
        io.flags |= wasixv2_abi::OpenFlags::O_NONBLOCK;
    }
    io
}

impl Pollable for SocketFile {
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        self.socket.poll(table)
    }
}

impl FileObject for SocketFile {
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move { Ok(self.recv(io, buf, MsgFlags(0)).await?.len) })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        let msg = SendMsg {
            data,
            to: None,
            flags: MsgFlags(0),
            control: &[],
        };
        Box::pin(self.send(io, msg))
    }

    /// `SIOCINQ` (`FIONREAD`) and `SIOCOUTQ` (`TIOCOUTQ`).
    fn ioctl<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        request: IoctlRequest,
        arg: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async move {
            let len = match request {
                IoctlRequest::FIONREAD => self.socket.input_len()?,
                IoctlRequest::TIOCOUTQ => self.socket.output_len()?,
                _ => return Err(Errno::ENOTTY.into()),
            };
            write_value(caller.memory(), GuestPtrMut::new(arg), &(len as i32)).await?;
            Ok(0)
        })
    }
}

/// The address family in the first two bytes of a guest sockaddr; `EINVAL` if shorter.
pub(crate) fn family(raw: &[u8]) -> Result<AddressFamily, Errno> {
    let bytes = raw.get(..2).ok_or(Errno::EINVAL)?;
    Ok(AddressFamily(
        u16::from_le_bytes([bytes[0], bytes[1]]).into(),
    ))
}

/// Decodes a guest sockaddr for a socket of `domain`, as `inet_bind`, `tcp_v4_connect` and
/// friends check it: too short is `EINVAL`, a family that does not fit `EAFNOSUPPORT`.
/// `unspec_any` accepts `AF_UNSPEC` as `AF_INET` (bind with `INADDR_ANY`, sendto).
pub(crate) fn decode(domain: Domain, raw: &[u8], unspec_any: bool) -> Result<SocketAddr, Errno> {
    match domain {
        Domain::Inet => {
            if raw.len() < SOCKADDR_IN {
                return Err(Errno::EINVAL);
            }
            let addr = decode_v4(raw);
            match family(raw)? {
                AddressFamily::AF_INET => Ok(addr),
                AddressFamily::AF_UNSPEC if unspec_any => Ok(addr),
                _ => Err(Errno::EAFNOSUPPORT),
            }
        }
        Domain::Inet6 => {
            if raw.len() < SOCKADDR_IN6_MIN {
                return Err(Errno::EINVAL);
            }
            match family(raw)? {
                AddressFamily::AF_INET6 => Ok(decode_v6(raw)),
                _ => Err(Errno::EAFNOSUPPORT),
            }
        }
    }
}

fn decode_v4(raw: &[u8]) -> SocketAddr {
    let port = u16::from_be_bytes([raw[2], raw[3]]);
    let ip = Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]);
    SocketAddr::V4(SocketAddrV4::new(ip, port))
}

fn decode_v6(raw: &[u8]) -> SocketAddr {
    let port = u16::from_be_bytes([raw[2], raw[3]]);
    let flowinfo = u32::from_be_bytes(raw[4..8].try_into().unwrap());
    let ip: [u8; 16] = raw[8..24].try_into().unwrap();
    let scope_id = match raw.get(24..SOCKADDR_IN6) {
        Some(bytes) => u32::from_le_bytes(bytes.try_into().unwrap()),
        None => 0,
    };
    SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::from(ip),
        port,
        flowinfo,
        scope_id,
    ))
}

/// A guest `sockaddr_in` or `sockaddr_in6`.
pub(crate) fn encode(addr: SocketAddr) -> Vec<u8> {
    match addr {
        SocketAddr::V4(addr) => {
            let mut raw = vec![0; SOCKADDR_IN];
            raw[..2].copy_from_slice(&(AddressFamily::AF_INET.0 as u16).to_le_bytes());
            raw[2..4].copy_from_slice(&addr.port().to_be_bytes());
            raw[4..8].copy_from_slice(&addr.ip().octets());
            raw
        }
        SocketAddr::V6(addr) => {
            let mut raw = vec![0; SOCKADDR_IN6];
            raw[..2].copy_from_slice(&(AddressFamily::AF_INET6.0 as u16).to_le_bytes());
            raw[2..4].copy_from_slice(&addr.port().to_be_bytes());
            raw[4..8].copy_from_slice(&addr.flowinfo().to_be_bytes());
            raw[8..24].copy_from_slice(&addr.ip().octets());
            raw[24..28].copy_from_slice(&addr.scope_id().to_le_bytes());
            raw
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockaddrs_round_trip() {
        let v4: SocketAddr = "10.0.0.1:8080".parse().unwrap();
        assert_eq!(decode(Domain::Inet, &encode(v4), false), Ok(v4));
        let v6 = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 443, 7, 3));
        assert_eq!(decode(Domain::Inet6, &encode(v6), false), Ok(v6));
        assert_eq!(
            decode(Domain::Inet6, &encode(v6)[..24], false)
                .unwrap()
                .port(),
            443
        );

        assert_eq!(
            decode(Domain::Inet, &encode(v4)[..15], false),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            decode(Domain::Inet6, &encode(v4), false),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            decode(Domain::Inet, &encode(v6), false),
            Err(Errno::EAFNOSUPPORT)
        );
        let mut unspec = encode(v4);
        unspec[..2].fill(0);
        assert_eq!(
            decode(Domain::Inet, &unspec, false),
            Err(Errno::EAFNOSUPPORT)
        );
        assert_eq!(decode(Domain::Inet, &unspec, true), Ok(v4));
    }
}
