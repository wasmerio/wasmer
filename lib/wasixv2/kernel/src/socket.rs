//! The socket layer every family shares, as Linux's `struct sock` generic parts: the
//! `SO_RCVTIMEO`/`SO_SNDTIMEO` timeouts and blocking built on them (`sk_wait_event`,
//! `sock_intr_errno`). Then IP sockets as open-file objects (a `wasixv2_vnet::Socket` plus
//! that layer), and the guest `sockaddr` codec.

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::{Arc, Mutex},
    time::Duration,
};

use wasixv2_abi::{AddressFamily, GuestPtrMut, IoctlRequest, MsgFlags};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysErr, SysResult, write_value};
use wasixv2_vnet::{Domain, RecvMsg, SendMsg, Socket, SocketKind};
use wasixv2_wait::{PollMask, PollTable, PollWaiter, Pollable, WaitQueue, Wakeup};

use crate::file::{FileObject, Io, ReadSink, delivered, taken};

/// `sizeof(struct sockaddr_storage)`, the most `move_addr_to_kernel` takes.
pub(crate) const SOCKADDR_STORAGE: u32 = 128;
const SOCKADDR_IN: usize = 16;
/// `SIN6_LEN_RFC2133`: an IPv6 address without the scope id is still accepted.
const SOCKADDR_IN6_MIN: usize = 24;
const SOCKADDR_IN6: usize = 28;

pub(crate) const RCV: usize = 0;
pub(crate) const SND: usize = 1;

/// `sk_rcvtimeo` and `sk_sndtimeo` (indexed by [`RCV`] and [`SND`]); `None` waits forever.
/// Every socket family has them.
#[derive(Default)]
pub(crate) struct Timeouts(Mutex<[Option<Duration>; 2]>);

impl Timeouts {
    pub(crate) fn get(&self, dir: usize) -> Option<Duration> {
        self.0.lock().unwrap()[dir]
    }

    pub(crate) fn set(&self, dir: usize, timeout: Option<Duration>) {
        self.0.lock().unwrap()[dir] = timeout;
    }
}

/// What a blocked socket call sleeps on.
#[derive(Clone, Copy)]
pub(crate) enum WaitOn<'a> {
    /// The queues `poll` registers on, for the events asked for.
    Object(&'a dyn Pollable),
    /// One queue, for any wakeup.
    Queue(&'a WaitQueue),
}

/// One socket call's blocking: the `timeo` it computed once at the start (`sock_rcvtimeo`,
/// `sock_sndtimeo`), counted down across every wait the call makes, as `tcp_sendmsg` and
/// `tcp_recvmsg` do with `sk_stream_wait_memory` and `sk_wait_data`.
pub(crate) struct Blocking<'a> {
    io: Io<'a>,
    timed: bool,
    timer: Option<LocalFuture<'a, ()>>,
    expired: bool,
}

impl<'a> Blocking<'a> {
    /// `MSG_DONTWAIT` in `flags` makes the call non-blocking.
    pub(crate) fn new(
        host: &'a dyn Host,
        io: Io<'a>,
        flags: MsgFlags,
        timeout: Option<Duration>,
    ) -> Self {
        let mut io = io;
        if flags.contains(MsgFlags::MSG_DONTWAIT) {
            io.flags |= wasixv2_abi::OpenFlags::O_NONBLOCK;
        }
        let timer = match io.nonblocking() {
            true => None,
            false => timeout.map(|timeout| host.sleep(timeout)),
        };
        Self {
            io,
            timed: timeout.is_some(),
            timer,
            expired: false,
        }
    }

    pub(crate) fn nonblocking(&self) -> bool {
        self.io.nonblocking()
    }

    /// `sk_wait_event` around a non-blocking operation: retries `op` while it fails with
    /// `EAGAIN`, sleeping on `on` for one of `events` in between. The timeout running out
    /// ends the wait with `EAGAIN`; a signal ends it with a restart, or with `EINTR` when a
    /// timeout was set (`sock_intr_errno`).
    pub(crate) async fn wait<T>(
        &mut self,
        on: WaitOn<'_>,
        events: PollMask,
        mut op: impl FnMut() -> Result<T, Errno>,
    ) -> SysResult<T> {
        let queue;
        let (object, key): (&dyn Pollable, PollMask) = match on {
            WaitOn::Object(object) => (object, events | PollMask::ERR | PollMask::HUP),
            WaitOn::Queue(q) => {
                queue = QueueOnly(q);
                (&queue, PollMask::empty())
            }
        };
        let mut waiter = PollWaiter::new();
        let mut registered = false;
        loop {
            match op() {
                Err(Errno::EAGAIN) if !self.io.nonblocking() && !self.expired => {}
                result => return Ok(result?),
            }
            if !registered {
                // On the queues before the retry, so no wakeup is lost.
                waiter.poll(object, key, true);
                registered = true;
                continue;
            }
            if self.io.interrupt.pending() {
                return Err(self.interrupted());
            }
            let timer = self.timer.as_mut().map(|timer| timer.as_mut());
            match waiter.sleep(self.io.interrupt, timer).await {
                Wakeup::Woken(_) => {}
                Wakeup::TimedOut => {
                    self.expired = true;
                    return Err(Errno::EAGAIN.into());
                }
                Wakeup::Interrupted => return Err(self.interrupted()),
            }
        }
    }

    fn interrupted(&self) -> SysErr {
        match self.timed {
            true => SysErr::Errno(Errno::EINTR),
            false => SysErr::Restart,
        }
    }
}

/// A bare queue as something to poll: registers on it and reports nothing.
struct QueueOnly<'a>(&'a WaitQueue);

impl Pollable for QueueOnly<'_> {
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        wasixv2_wait::poll_wait(&mut table, self.0);
        PollMask::empty()
    }
}

/// The kernel's side of an IP socket.
pub(crate) struct SocketFile {
    pub(crate) socket: Arc<dyn Socket>,
    host: Arc<dyn Host>,
    pub(crate) timeouts: Timeouts,
    /// Held by a stream reader from peeking at the data until it took it (`lock_sock` in
    /// `tcp_recvmsg`), so what it leaves behind on a fault is still first.
    /// Never held while waiting for data.
    io_lock: futures_util::lock::Mutex<()>,
}

impl SocketFile {
    pub(crate) fn new(socket: Arc<dyn Socket>, host: Arc<dyn Host>) -> Self {
        Self {
            socket,
            host,
            timeouts: Timeouts::default(),
            io_lock: futures_util::lock::Mutex::new(()),
        }
    }

    /// The blocking of one call in direction `dir`.
    pub(crate) fn blocking<'a>(&'a self, io: Io<'a>, dir: usize, flags: MsgFlags) -> Blocking<'a> {
        Blocking::new(&*self.host, io, flags, self.timeouts.get(dir))
    }

    /// One receive into `buf`, waiting for data unless non-blocking. `MSG_WAITALL` is the
    /// caller's to loop on.
    pub(crate) async fn recv(
        &self,
        wait: &mut Blocking<'_>,
        buf: &mut [u8],
        flags: MsgFlags,
    ) -> SysResult<RecvMsg> {
        let socket = &self.socket;
        let proto_flags = flags & !(MsgFlags::MSG_WAITALL | MsgFlags::MSG_DONTWAIT);
        wait.wait(WaitOn::Object(&**socket), PollMask::READABLE, || {
            socket.recv(buf, proto_flags)
        })
        .await
    }

    /// A stream receive into `to` through `buf` that takes the data only once all of it
    /// arrived, as `tcp_recvmsg` moves `copied_seq` past an skb only once
    /// `skb_copy_datagram_msg` copied it: a fault is `EFAULT` with everything still queued
    /// ([`taken`]). Waits without the I/O lock (`sk_wait_data` releases the socket), then
    /// peeks under it, copies and takes.
    pub(crate) async fn recv_stream_to(
        &self,
        wait: &mut Blocking<'_>,
        buf: &mut [u8],
        flags: MsgFlags,
        to: &mut dyn ReadSink,
    ) -> SysResult<RecvMsg> {
        let socket = &self.socket;
        let flags = flags & !(MsgFlags::MSG_WAITALL | MsgFlags::MSG_DONTWAIT);
        let peek = flags | MsgFlags::MSG_PEEK;
        if buf.is_empty() {
            return Ok(socket.recv(buf, flags)?);
        }
        loop {
            let mut probe = [0];
            let ready = wait
                .wait(WaitOn::Object(&**socket), PollMask::READABLE, || {
                    socket.recv(&mut probe, peek)
                })
                .await?;
            if ready.len == 0 {
                return Ok(ready);
            }
            let _io = self.io_lock.lock().await;
            let look = match socket.recv(buf, peek) {
                Err(Errno::EAGAIN) => continue,
                Err(err) => return Err(err.into()),
                Ok(look) if look.len == 0 => return Ok(look),
                Ok(look) => look,
            };
            let n = taken(look.len, to.copy_out(&buf[..look.len]).await)?;
            let got = socket.recv(&mut buf[..n], flags)?;
            debug_assert_eq!(got.len, n, "the I/O lock keeps the data in place");
            return Ok(got);
        }
    }

    /// Sends `msg`, waiting for room unless non-blocking. A blocking stream send goes on until
    /// everything is sent, returning the partial count if it then fails.
    pub(crate) async fn send(&self, wait: &mut Blocking<'_>, msg: SendMsg<'_>) -> SysResult<usize> {
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
            match wait
                .wait(WaitOn::Object(&**socket), PollMask::WRITABLE, || {
                    socket.send(part)
                })
                .await
            {
                Ok(n) => sent += n,
                Err(_) if sent > 0 => return Ok(sent),
                Err(err) => return Err(err),
            }
            if !stream || wait.nonblocking() || sent >= msg.data.len() {
                return Ok(sent);
            }
        }
    }
}

impl Pollable for SocketFile {
    fn poll(&self, table: Option<&mut PollTable<'_>>) -> PollMask {
        self.socket.poll(table)
    }
}

impl FileObject for SocketFile {
    fn messages(&self) -> bool {
        self.socket.kind() != wasixv2_vnet::SocketKind::Stream
    }

    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.blocking(io, RCV, MsgFlags(0));
            Ok(self.recv(&mut wait, buf, MsgFlags(0)).await?.len)
        })
    }

    fn read_to<'a>(
        &'a self,
        io: Io<'a>,
        buf: &'a mut [u8],
        to: &'a mut dyn ReadSink,
    ) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let mut wait = self.blocking(io, RCV, MsgFlags(0));
            if self.socket.kind() != SocketKind::Stream {
                let n = self.recv(&mut wait, buf, MsgFlags(0)).await?.len;
                return delivered(n, to.copy_out(&buf[..n]).await);
            }
            let got = self.recv_stream_to(&mut wait, buf, MsgFlags(0), to).await?;
            Ok(got.len)
        })
    }

    fn write<'a>(&'a self, io: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        let msg = SendMsg {
            data,
            to: None,
            flags: MsgFlags(0),
            control: &[],
            net_raw: false,
        };
        Box::pin(async move {
            let mut wait = self.blocking(io, SND, MsgFlags(0));
            self.send(&mut wait, msg).await
        })
    }

    fn buffered(&self) -> usize {
        self.socket.input_len().unwrap_or(0)
    }

    /// `sock_splice_read`.
    fn splice_read(&self) -> bool {
        true
    }

    /// `splice_to_socket`.
    fn splice_write(&self) -> bool {
        true
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

/// A datagram send's destination, as `udp_sendmsg` and `udpv6_sendmsg` read it: on an IPv6
/// socket `AF_INET` goes the IPv4 way and `AF_UNSPEC` means no address (the connected peer).
pub(crate) fn decode_dest(domain: Domain, raw: &[u8]) -> Result<Option<SocketAddr>, Errno> {
    if domain == Domain::Inet {
        return decode(domain, raw, true).map(Some);
    }
    match family(raw)? {
        AddressFamily::AF_INET6 => decode(domain, raw, false).map(Some),
        AddressFamily::AF_INET if raw.len() >= SOCKADDR_IN => Ok(Some(decode_v4(raw))),
        AddressFamily::AF_UNSPEC => Ok(None),
        _ => Err(Errno::EINVAL),
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
