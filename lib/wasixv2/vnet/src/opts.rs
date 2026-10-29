//! Socket options shared by TCP and UDP sockets (`sk_setsockopt`/`sk_getsockopt`,
//! `do_ip_setsockopt`, `do_ipv6_setsockopt`, `do_tcp_setsockopt`). Values arrive as raw guest
//! bytes, little-endian as on wasm32, so option semantics live here and not in the kernel.
//! Timeouts and the read-only descriptor options are the kernel's (see `api::Socket::set_option`).

use std::collections::BTreeMap;

use wasixv2_abi::{Errno, IpOption, Ipv6Option, SocketLevel, SocketOption as So, TcpOption as Tcp};

use crate::{
    Domain, NetResult, SocketKind,
    mcast::{self, Mcast},
    net::Net,
};

/// `IPV6_V6ONLY`; missing from the generated ABI constants.
pub const IPV6_V6ONLY: i32 = 26;

/// `sysctl_wmem_max` / `sysctl_rmem_max` defaults.
pub const WMEM_MAX: u32 = 212_992;
pub const RMEM_MAX: u32 = 212_992;
/// `SOCK_MIN_SNDBUF` / `SOCK_MIN_RCVBUF` on 64-bit Linux.
pub const SOCK_MIN_SNDBUF: usize = 4608;
pub const SOCK_MIN_RCVBUF: usize = 2304;
/// `tcp_wmem[1]`, `tcp_rmem[1]`.
pub const TCP_SNDBUF_DEFAULT: usize = 16384;
pub const TCP_RCVBUF_DEFAULT: usize = 131_072;
/// `wmem_default`, `rmem_default` (UDP).
pub const UDP_BUF_DEFAULT: usize = 212_992;
/// `TCP_MSS_DEFAULT` and the MSS of a loopback connection (MTU 65536 with timestamps).
pub const TCP_MSS_DEFAULT: i32 = 536;
pub const TCP_MSS_LOOPBACK: i32 = 65483;

const SOL_SOCKET: i32 = SocketLevel::SOL_SOCKET.0;
const SOL_IP: i32 = SocketLevel::SOL_IP.0;
const SOL_IPV6: i32 = SocketLevel::SOL_IPV6.0;
const SOL_TCP: i32 = SocketLevel::SOL_TCP.0;

/// What a successful `setsockopt` changed that the socket must act on.
#[derive(Debug, PartialEq, Eq)]
pub enum Changed {
    Nothing,
    /// `SO_REUSEADDR`/`SO_REUSEPORT`: update the port table entry.
    Reuse,
    /// `SO_SNDBUF`/`SO_RCVBUF`/`SO_RCVLOWAT`/`SO_OOBINLINE`: buffers, readiness thresholds or
    /// how urgent bytes are read changed.
    Buffers,
}

/// Per-socket option state.
#[derive(Clone, Debug)]
pub struct SockOpts {
    pub ty: SocketKind,
    /// The IP family; `None` for a Unix socket, which only has the `SOL_SOCKET` options.
    pub family: Option<Domain>,
    pub reuseaddr: bool,
    pub reuseport: bool,
    pub v6only: bool,
    pub sndbuf: usize,
    pub rcvbuf: usize,
    pub rcvlowat: usize,
    /// `SO_LINGER`: `Some(secs)` when enabled.
    pub linger: Option<i32>,
    pub bind_device: Vec<u8>,
    pub congestion: Vec<u8>,
    pub user_mss: i32,
    /// Multicast options and memberships.
    pub(crate) mc: Mcast,
    /// Plain integer options keyed by (level, name).
    ints: BTreeMap<(i32, i32), i32>,
}

/// Plain integer options: (level, name, default, boolean, stream-only).
const INT_OPTS: &[(i32, i32, i32, bool, bool)] = &[
    (SOL_SOCKET, So::SO_DEBUG.0, 0, true, false),
    (SOL_SOCKET, So::SO_DONTROUTE.0, 0, true, false),
    (SOL_SOCKET, So::SO_BROADCAST.0, 0, true, false),
    (SOL_SOCKET, So::SO_KEEPALIVE.0, 0, true, false),
    (SOL_SOCKET, So::SO_OOBINLINE.0, 0, true, false),
    (SOL_SOCKET, So::SO_NO_CHECK.0, 0, true, false),
    (SOL_SOCKET, So::SO_PASSCRED.0, 0, true, false),
    (SOL_SOCKET, So::SO_TIMESTAMP_OLD.0, 0, true, false),
    (SOL_SOCKET, So::SO_TIMESTAMP_NEW.0, 0, true, false),
    (SOL_SOCKET, So::SO_TIMESTAMPNS_OLD.0, 0, true, false),
    (SOL_SOCKET, So::SO_TIMESTAMPNS_NEW.0, 0, true, false),
    (SOL_SOCKET, So::SO_RXQ_OVFL.0, 0, true, false),
    (SOL_SOCKET, So::SO_SELECT_ERR_QUEUE.0, 0, true, false),
    (SOL_SOCKET, So::SO_ZEROCOPY.0, 0, true, false),
    (SOL_SOCKET, So::SO_BUSY_POLL.0, 0, false, false),
    (SOL_SOCKET, So::SO_INCOMING_CPU.0, -1, false, false),
    (SOL_SOCKET, So::SO_PRIORITY.0, 0, false, false),
    (SOL_IP, IpOption::IP_TOS.0, 0, false, false),
    (SOL_IP, IpOption::IP_RECVERR.0, 0, true, false),
    (SOL_IP, IpOption::IP_PKTINFO.0, 0, true, false),
    (SOL_IP, IpOption::IP_RECVTOS.0, 0, true, false),
    (SOL_IP, IpOption::IP_RECVTTL.0, 0, true, false),
    (SOL_IP, IpOption::IP_RECVORIGDSTADDR.0, 0, true, false),
    (SOL_IP, IpOption::IP_MTU_DISCOVER.0, 1, false, false),
    (SOL_IP, IpOption::IP_FREEBIND.0, 0, true, false),
    (SOL_IP, IpOption::IP_BIND_ADDRESS_NO_PORT.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_RECVPKTINFO.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_TCLASS.0, 0, false, false),
    (SOL_IPV6, Ipv6Option::IPV6_RECVTCLASS.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_RECVERR.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_DONTFRAG.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_FREEBIND.0, 0, true, false),
    (SOL_IPV6, Ipv6Option::IPV6_MTU_DISCOVER.0, 1, false, false),
    (SOL_IPV6, Ipv6Option::IPV6_RECVHOPLIMIT.0, 0, true, false),
    (SOL_TCP, Tcp::TCP_NODELAY.0, 0, true, true),
    (SOL_TCP, Tcp::TCP_CORK.0, 0, true, true),
    (SOL_TCP, Tcp::TCP_KEEPIDLE.0, 7200, false, true),
    (SOL_TCP, Tcp::TCP_KEEPINTVL.0, 75, false, true),
    (SOL_TCP, Tcp::TCP_KEEPCNT.0, 9, false, true),
    (SOL_TCP, Tcp::TCP_SYNCNT.0, 6, false, true),
    (SOL_TCP, Tcp::TCP_DEFER_ACCEPT.0, 0, false, true),
    (SOL_TCP, Tcp::TCP_WINDOW_CLAMP.0, 0, false, true),
    (SOL_TCP, Tcp::TCP_QUICKACK.0, 1, true, true),
    (SOL_TCP, Tcp::TCP_USER_TIMEOUT.0, 0, false, true),
    (SOL_TCP, Tcp::TCP_FASTOPEN.0, 0, false, true),
    (SOL_TCP, Tcp::TCP_FASTOPEN_CONNECT.0, 0, true, true),
    (SOL_TCP, Tcp::TCP_NOTSENT_LOWAT.0, 0, false, true),
    (SOL_TCP, Tcp::TCP_THIN_LINEAR_TIMEOUTS.0, 0, true, true),
    (SOL_TCP, Tcp::TCP_INQ.0, 0, true, true),
    (SOL_TCP, 8 /* TCP_LINGER2 */, 60, false, true),
];

/// The integer options a backend stream acts on, in the order they are replayed: keepalive's
/// parameters before keepalive itself.
const BACKEND_INTS: &[(i32, i32)] = &[
    (SOL_SOCKET, So::SO_OOBINLINE.0),
    (SOL_TCP, Tcp::TCP_NODELAY.0),
    (SOL_TCP, Tcp::TCP_KEEPIDLE.0),
    (SOL_TCP, Tcp::TCP_KEEPINTVL.0),
    (SOL_TCP, Tcp::TCP_KEEPCNT.0),
    (SOL_SOCKET, So::SO_KEEPALIVE.0),
];

/// Reads an `int` option value (`copy_safe_from_sockptr` with `sizeof(int)`).
pub fn read_int(value: &[u8]) -> NetResult<i32> {
    let bytes: [u8; 4] = value
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .ok_or(Errno::EINVAL)?;
    Ok(i32::from_le_bytes(bytes))
}

/// Writes an `int` result, truncated to the caller's length as `sk_getsockopt` does.
pub fn write_int(out: &mut [u8], v: i32) -> usize {
    write_bytes(out, &v.to_le_bytes())
}

pub fn write_bytes(out: &mut [u8], v: &[u8]) -> usize {
    let n = out.len().min(v.len());
    out[..n].copy_from_slice(&v[..n]);
    n
}

/// Buffer size after `SO_SNDBUF`/`SO_RCVBUF`: clamp to the sysctl maximum, then double to
/// account for bookkeeping overhead, as `sk_setsockopt` does.
fn buf_size(val: i32, max: u32, min: usize) -> usize {
    let v = (val as u32).min(max) as usize;
    (v * 2).max(min)
}

/// `sk_sndbuf` after `setsockopt(SO_SNDBUF, val)`, for every socket family.
pub fn sndbuf_from(val: i32) -> usize {
    buf_size(val, WMEM_MAX, SOCK_MIN_SNDBUF)
}

/// `sk_rcvbuf` after `setsockopt(SO_RCVBUF, val)`, for every socket family.
pub fn rcvbuf_from(val: i32) -> usize {
    buf_size(val, RMEM_MAX, SOCK_MIN_RCVBUF)
}

impl SockOpts {
    pub fn new(ty: SocketKind, family: Domain) -> Self {
        let (sndbuf, rcvbuf) = match ty {
            SocketKind::Stream => (TCP_SNDBUF_DEFAULT, TCP_RCVBUF_DEFAULT),
            SocketKind::Datagram | SocketKind::Raw => (UDP_BUF_DEFAULT, UDP_BUF_DEFAULT),
        };
        Self {
            ty,
            family: Some(family),
            reuseaddr: false,
            reuseport: false,
            v6only: false,
            sndbuf,
            rcvbuf,
            rcvlowat: 1,
            linger: None,
            bind_device: Vec::new(),
            congestion: b"cubic".to_vec(),
            user_mss: 0,
            mc: Mcast::default(),
            ints: BTreeMap::new(),
        }
    }

    /// The `SOL_SOCKET` options of a Unix socket (`sk_setsockopt` for `PF_UNIX`), whose
    /// buffers default to `wmem_default` and `rmem_default`. `ty` is `Stream` for stream
    /// sockets and `Datagram` for datagram and seqpacket ones.
    pub fn unix(ty: SocketKind) -> Self {
        Self {
            family: None,
            sndbuf: UDP_BUF_DEFAULT,
            rcvbuf: UDP_BUF_DEFAULT,
            ..Self::new(ty, Domain::Inet)
        }
    }

    /// The option set an accepted socket inherits from its listener (`sk_clone_lock` copies
    /// the whole sock).
    pub fn inherit(&self) -> Self {
        self.clone()
    }

    /// The options a backend stream acts on (`TCP_NODELAY`, keepalive, `SO_LINGER`), as
    /// `(level, name, value)` in guest layout, for each one the guest set. Linux keeps them in
    /// the sock whatever its state, so a stream made later (`connect`, a published
    /// connection) gets them replayed.
    pub fn backend_options(&self) -> Vec<(i32, i32, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(secs) = self.linger {
            let mut v = 1i32.to_le_bytes().to_vec();
            v.extend(secs.to_le_bytes());
            out.push((SOL_SOCKET, So::SO_LINGER.0, v));
        }
        for &(level, name) in BACKEND_INTS {
            if let Some(v) = self.ints.get(&(level, name)) {
                out.push((level, name, v.to_le_bytes().to_vec()));
            }
        }
        out
    }

    fn int_opt(&self, level: i32, name: i32) -> Option<(i32, bool)> {
        INT_OPTS
            .iter()
            .find(|o| o.0 == level && o.1 == name && (!o.4 || self.ty == SocketKind::Stream))
            .map(|o| (self.ints.get(&(level, name)).copied().unwrap_or(o.2), o.3))
    }

    pub fn int(&self, level: i32, name: i32) -> i32 {
        self.int_opt(level, name).map_or(0, |o| o.0)
    }

    pub fn flag(&self, level: i32, name: i32) -> bool {
        self.int(level, name) != 0
    }

    /// Checks that `level` applies to this socket at all.
    fn check_level(&self, level: i32) -> NetResult<()> {
        match (level, self.family) {
            (SOL_SOCKET, _) | (SOL_IP, Some(_)) => Ok(()),
            (SOL_IPV6, Some(Domain::Inet6)) => Ok(()),
            (SOL_TCP, Some(_)) if self.ty == SocketKind::Stream => Ok(()),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// `setsockopt` of a multicast option ([`mcast::handles`]), which needs the namespace's
    /// interfaces.
    pub(crate) fn set_mcast(
        &mut self,
        net: &Net,
        level: i32,
        name: i32,
        value: &[u8],
    ) -> NetResult<()> {
        self.check_level(level)?;
        let bound_dev = net.device_index(&self.bind_device);
        self.mc.set(net, self.ty, bound_dev, level, name, value)
    }

    /// `setsockopt` for the options every inet socket shares. `bound` is whether the socket
    /// holds a port (some options are fixed after bind).
    pub fn set(&mut self, level: i32, name: i32, value: &[u8], bound: bool) -> NetResult<Changed> {
        self.check_level(level)?;
        match (level, name) {
            (SOL_SOCKET, n) if n == So::SO_LINGER.0 => {
                if value.len() < 8 {
                    return Err(Errno::EINVAL);
                }
                let onoff = read_int(&value[..4])?;
                let secs = read_int(&value[4..8])?;
                self.linger = (onoff != 0).then_some(secs.max(0));
                return Ok(Changed::Nothing);
            }
            (SOL_SOCKET, n) if n == So::SO_BINDTODEVICE.0 => {
                let name = value.split(|b| *b == 0).next().unwrap_or_default();
                self.bind_device = name.to_vec();
                return Ok(Changed::Nothing);
            }
            (SOL_TCP, n) if n == Tcp::TCP_CONGESTION.0 => {
                let name = value.split(|b| *b == 0).next().unwrap_or_default();
                if name != b"cubic" && name != b"reno" {
                    return Err(Errno::ENOENT);
                }
                self.congestion = name.to_vec();
                return Ok(Changed::Nothing);
            }
            _ => {}
        }
        let val = read_int(value)?;
        match (level, name) {
            (SOL_SOCKET, n) if n == So::SO_REUSEADDR.0 => {
                self.reuseaddr = val != 0;
                Ok(Changed::Reuse)
            }
            (SOL_SOCKET, n) if n == So::SO_REUSEPORT.0 => {
                if val != 0 && self.family.is_none() {
                    return Err(Errno::EOPNOTSUPP);
                }
                self.reuseport = val != 0;
                Ok(Changed::Reuse)
            }
            // TCP and UDP sockets support zerocopy sends, other families do not.
            (SOL_SOCKET, n) if n == So::SO_ZEROCOPY.0 => {
                if self.family.is_none() {
                    return Err(Errno::EOPNOTSUPP);
                }
                if !(0..=1).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.ints.insert((level, name), val);
                Ok(Changed::Nothing)
            }
            (SOL_SOCKET, n) if n == So::SO_SNDBUF.0 => {
                self.sndbuf = buf_size(val, WMEM_MAX, SOCK_MIN_SNDBUF);
                Ok(Changed::Buffers)
            }
            (SOL_SOCKET, n) if n == So::SO_RCVBUF.0 => {
                self.rcvbuf = buf_size(val, RMEM_MAX, SOCK_MIN_RCVBUF);
                Ok(Changed::Buffers)
            }
            // TODO: the forcing variants, which ignore the maximum with CAP_NET_ADMIN.
            (SOL_SOCKET, n) if n == So::SO_SNDBUFFORCE.0 || n == So::SO_RCVBUFFORCE.0 => {
                Err(Errno::EPERM)
            }
            (SOL_SOCKET, n) if n == So::SO_OOBINLINE.0 => {
                self.ints.insert((level, name), (val != 0) as i32);
                Ok(Changed::Buffers)
            }
            (SOL_SOCKET, n) if n == So::SO_RCVLOWAT.0 => {
                self.rcvlowat = if val < 0 {
                    i32::MAX as usize
                } else {
                    val.max(1) as usize
                };
                Ok(Changed::Buffers)
            }
            (SOL_SOCKET, n)
                if n == So::SO_TYPE.0
                    || n == So::SO_ERROR.0
                    || n == So::SO_PROTOCOL.0
                    || n == So::SO_DOMAIN.0
                    || n == So::SO_ACCEPTCONN.0
                    || n == So::SO_SNDLOWAT.0
                    || n == So::SO_PEERNAME.0 =>
            {
                Err(Errno::ENOPROTOOPT)
            }
            (SOL_IP, n) if n == IpOption::IP_TTL.0 => {
                if val != -1 && !(1..=255).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.ints.insert((level, name), val);
                Ok(Changed::Nothing)
            }
            (SOL_IPV6, n) if n == Ipv6Option::IPV6_UNICAST_HOPS.0 => {
                if !(-1..=255).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.ints.insert((level, name), val);
                Ok(Changed::Nothing)
            }
            (SOL_IPV6, IPV6_V6ONLY) => {
                if bound {
                    return Err(Errno::EINVAL);
                }
                self.v6only = val != 0;
                Ok(Changed::Nothing)
            }
            (SOL_TCP, n) if n == Tcp::TCP_MAXSEG.0 => {
                if val != 0 && !(88..=65535).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.user_mss = val;
                Ok(Changed::Nothing)
            }
            (SOL_TCP, n) if n == Tcp::TCP_KEEPIDLE.0 || n == Tcp::TCP_KEEPINTVL.0 => {
                if !(1..=32767).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.ints.insert((level, name), val);
                Ok(Changed::Nothing)
            }
            (SOL_TCP, n) if n == Tcp::TCP_KEEPCNT.0 || n == Tcp::TCP_SYNCNT.0 => {
                if !(1..=127).contains(&val) {
                    return Err(Errno::EINVAL);
                }
                self.ints.insert((level, name), val);
                Ok(Changed::Nothing)
            }
            (SOL_TCP, n) if n == Tcp::TCP_USER_TIMEOUT.0 && val < 0 => Err(Errno::EINVAL),
            _ => match self.int_opt(level, name) {
                Some((_, boolean)) => {
                    self.ints
                        .insert((level, name), if boolean { (val != 0) as i32 } else { val });
                    Ok(Changed::Nothing)
                }
                None => Err(Errno::ENOPROTOOPT),
            },
        }
    }

    /// `getsockopt` for the shared options; socket-specific ones (`SO_ERROR`, `SO_ACCEPTCONN`,
    /// `TCP_INFO`, ...) are answered by the socket first.
    pub fn get(&self, level: i32, name: i32, out: &mut [u8]) -> NetResult<usize> {
        self.check_level(level)?;
        if mcast::handles(level, name) {
            return self.mc.get(self.ty, level, name, out);
        }
        let v = match (level, name) {
            (SOL_SOCKET, n) if n == So::SO_LINGER.0 => {
                let mut buf = [0u8; 8];
                buf[..4].copy_from_slice(&(self.linger.is_some() as i32).to_le_bytes());
                buf[4..].copy_from_slice(&self.linger.unwrap_or(0).to_le_bytes());
                return Ok(write_bytes(out, &buf));
            }
            (SOL_SOCKET, n) if n == So::SO_BINDTODEVICE.0 => {
                let mut name = self.bind_device.clone();
                name.push(0);
                if out.len() < name.len() {
                    return Err(Errno::EINVAL);
                }
                return Ok(write_bytes(out, &name));
            }
            (SOL_TCP, n) if n == Tcp::TCP_CONGESTION.0 => {
                let mut name = [0u8; 16];
                name[..self.congestion.len()].copy_from_slice(&self.congestion);
                return Ok(write_bytes(out, &name));
            }
            (SOL_SOCKET, n) if n == So::SO_REUSEADDR.0 => self.reuseaddr as i32,
            (SOL_SOCKET, n) if n == So::SO_REUSEPORT.0 => self.reuseport as i32,
            (SOL_SOCKET, n) if n == So::SO_SNDBUF.0 => self.sndbuf as i32,
            (SOL_SOCKET, n) if n == So::SO_RCVBUF.0 => self.rcvbuf as i32,
            (SOL_SOCKET, n) if n == So::SO_RCVLOWAT.0 => self.rcvlowat as i32,
            (SOL_SOCKET, n) if n == So::SO_SNDLOWAT.0 => 1,
            (SOL_SOCKET, n) if n == So::SO_TYPE.0 => match self.ty {
                SocketKind::Stream => 1,
                SocketKind::Datagram => 2,
                SocketKind::Raw => 3,
            },
            (SOL_SOCKET, n) if n == So::SO_PROTOCOL.0 => match (self.family, self.ty) {
                (None, _) => 0,
                (Some(_), SocketKind::Stream) => 6,
                (Some(_), SocketKind::Datagram) => 17,
                // The socket's own protocol, which the kernel reports before asking.
                (Some(_), SocketKind::Raw) => 255,
            },
            (SOL_SOCKET, n) if n == So::SO_DOMAIN.0 => self.family.map_or(1, Domain::raw),
            (SOL_IP, n) if n == IpOption::IP_TTL.0 => match self.int(level, name) {
                0 | -1 => 64,
                v => v,
            },
            (SOL_IPV6, n) if n == Ipv6Option::IPV6_UNICAST_HOPS.0 => {
                match self.ints.get(&(level, name)) {
                    None | Some(-1) => 64,
                    Some(v) => *v,
                }
            }
            (SOL_IPV6, IPV6_V6ONLY) => self.v6only as i32,
            _ => match self.int_opt(level, name) {
                Some((v, _)) => v,
                None => return Err(Errno::ENOPROTOOPT),
            },
        };
        Ok(write_int(out, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(o: &SockOpts, level: i32, name: i32) -> i32 {
        let mut b = [0u8; 4];
        o.get(level, name, &mut b).unwrap();
        i32::from_le_bytes(b)
    }

    #[test]
    fn buffer_sizes_are_doubled_and_clamped() {
        let mut o = SockOpts::new(SocketKind::Stream, Domain::Inet);
        assert_eq!(get(&o, SOL_SOCKET, So::SO_RCVBUF.0), 131_072);
        o.set(SOL_SOCKET, So::SO_RCVBUF.0, &1000i32.to_le_bytes(), false)
            .unwrap();
        assert_eq!(get(&o, SOL_SOCKET, So::SO_RCVBUF.0), 2304);
        o.set(SOL_SOCKET, So::SO_SNDBUF.0, &10_000i32.to_le_bytes(), false)
            .unwrap();
        assert_eq!(get(&o, SOL_SOCKET, So::SO_SNDBUF.0), 20_000);
        o.set(SOL_SOCKET, So::SO_SNDBUF.0, &(-1i32).to_le_bytes(), false)
            .unwrap();
        assert_eq!(get(&o, SOL_SOCKET, So::SO_SNDBUF.0), 2 * 212_992);
    }

    #[test]
    fn level_and_validation_errors() {
        let mut o = SockOpts::new(SocketKind::Datagram, Domain::Inet);
        let one = 1i32.to_le_bytes();
        assert_eq!(
            o.set(SOL_TCP, Tcp::TCP_NODELAY.0, &one, false),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(
            o.set(SOL_IPV6, IPV6_V6ONLY, &one, false),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(
            o.set(SOL_SOCKET, So::SO_REUSEADDR.0, &[1, 0], false),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            o.set(SOL_SOCKET, So::SO_TYPE.0, &one, false),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(
            o.set(SOL_SOCKET, 9999, &one, false),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(get(&o, SOL_SOCKET, So::SO_TYPE.0), 2);
        let mut six = SockOpts::new(SocketKind::Stream, Domain::Inet6);
        assert_eq!(
            six.set(SOL_IPV6, IPV6_V6ONLY, &one, true),
            Err(Errno::EINVAL)
        );
        six.set(SOL_IPV6, IPV6_V6ONLY, &one, false).unwrap();
        assert!(six.v6only);
        six.set(SOL_TCP, Tcp::TCP_NODELAY.0, &7i32.to_le_bytes(), false)
            .unwrap();
        assert_eq!(get(&six, SOL_TCP, Tcp::TCP_NODELAY.0), 1);
    }
}
