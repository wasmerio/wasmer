//! Socket options: `sock_set_opt_*`/`sock_get_opt_*` and the multicast calls.
//!
//! Each v1 [`Sockoption`] has one kind (flag, size or time, matching the call that takes it)
//! and maps to a Linux option, except `CONNECT_TIMEOUT` and `ACCEPT_TIMEOUT`, which Linux does
//! not have: the compat layer keeps them per socket and applies them in `sock_connect` and
//! `sock_accept`.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use wasixv2_abi::{
    ConstSockaddrPtr, FcntlCmd, Fd, GuestPtrMut, IpOption, Ipv6Option, KernelSockTimeval, Linger,
    OpenFlags, PollEvents, SocketLevel, SocketOption, SocketType, TcpOption,
};
use wasixv2_syscalls::{Control, SysErr, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Bool, Errno, GuestLayout,
        proc::{OptionTag, OptionTimestamp},
        sock::{AddrIp4, AddrIp6, Sockoption, Socktype},
    },
    errno::V1Result,
    shim::{self, Env, Frame},
};

use super::{poll::wait_fd, sock::int_option};

/// `IPV6_V6ONLY`, which the generated ABI constants lack.
const IPV6_V6ONLY: i32 = 26;

/// Which `sock_*_opt_*` call an option belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Flag,
    Size,
    Time,
}

/// Where an option lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A Linux option, as `(level, name)`.
    Linux(SocketLevel, i32),
    /// `SO_LINGER`, whose `struct linger` v1 passes as an optional time.
    Linger,
    /// A timeout the compat layer applies.
    Compat(Wait),
    /// `NOOP`: accepted and ignored.
    Noop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wait {
    Connect,
    Accept,
}

/// What an option is, or why it is not served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spec {
    Option(Kind, Target),
    /// Defined by v1, with nothing Linux can serve it with (`PROMISCUOUS`).
    Unsupported,
    /// Not a v1 option.
    Unknown,
}

fn spec(option: Sockoption) -> Spec {
    use Kind::{Flag, Size, Time};
    use Target::Linux;
    let sol = |name: SocketOption| Linux(SocketLevel::SOL_SOCKET, name.0);
    let ip = |name: IpOption| Linux(SocketLevel::SOL_IP, name.0);
    let (kind, target) = match option {
        Sockoption::NOOP => return Spec::Option(Flag, Target::Noop),
        Sockoption::REUSE_PORT => (Flag, sol(SocketOption::SO_REUSEPORT)),
        Sockoption::REUSE_ADDR => (Flag, sol(SocketOption::SO_REUSEADDR)),
        Sockoption::NO_DELAY => (Flag, Linux(SocketLevel::SOL_TCP, TcpOption::TCP_NODELAY.0)),
        Sockoption::DONT_ROUTE => (Flag, sol(SocketOption::SO_DONTROUTE)),
        Sockoption::ONLY_V6 => (Flag, Linux(SocketLevel::SOL_IPV6, IPV6_V6ONLY)),
        Sockoption::BROADCAST => (Flag, sol(SocketOption::SO_BROADCAST)),
        Sockoption::MULTICAST_LOOP_V4 => (Flag, ip(IpOption::IP_MULTICAST_LOOP)),
        Sockoption::MULTICAST_LOOP_V6 => (
            Flag,
            Linux(SocketLevel::SOL_IPV6, Ipv6Option::IPV6_MULTICAST_LOOP.0),
        ),
        Sockoption::PROMISCUOUS => return Spec::Unsupported,
        Sockoption::LISTENING => (Flag, sol(SocketOption::SO_ACCEPTCONN)),
        Sockoption::LAST_ERROR => (Size, sol(SocketOption::SO_ERROR)),
        Sockoption::KEEP_ALIVE => (Flag, sol(SocketOption::SO_KEEPALIVE)),
        Sockoption::LINGER => (Time, Target::Linger),
        Sockoption::OOB_INLINE => (Flag, sol(SocketOption::SO_OOBINLINE)),
        Sockoption::RECV_BUF_SIZE => (Size, sol(SocketOption::SO_RCVBUF)),
        Sockoption::SEND_BUF_SIZE => (Size, sol(SocketOption::SO_SNDBUF)),
        Sockoption::RECV_LOWAT => (Size, sol(SocketOption::SO_RCVLOWAT)),
        Sockoption::SEND_LOWAT => (Size, sol(SocketOption::SO_SNDLOWAT)),
        Sockoption::RECV_TIMEOUT => (Time, sol(SocketOption::SO_RCVTIMEO_NEW)),
        Sockoption::SEND_TIMEOUT => (Time, sol(SocketOption::SO_SNDTIMEO_NEW)),
        Sockoption::CONNECT_TIMEOUT => (Time, Target::Compat(Wait::Connect)),
        Sockoption::ACCEPT_TIMEOUT => (Time, Target::Compat(Wait::Accept)),
        Sockoption::TTL => (Size, ip(IpOption::IP_TTL)),
        Sockoption::MULTICAST_TTL_V4 => (Size, ip(IpOption::IP_MULTICAST_TTL)),
        Sockoption::TYPE => (Size, sol(SocketOption::SO_TYPE)),
        Sockoption::PROTO => (Size, sol(SocketOption::SO_PROTOCOL)),
        _ => return Spec::Unknown,
    };
    Spec::Option(kind, target)
}

/// The target of `option`, which a `kind` call must serve.
fn target(option: i32, kind: Kind) -> V1Result<Target> {
    match spec(Sockoption(option as u8)) {
        Spec::Option(Kind::Flag, Target::Noop) => Ok(Target::Noop),
        Spec::Option(k, target) if k == kind => Ok(target),
        Spec::Option(..) | Spec::Unknown => shim::fail(Errno::INVAL),
        Spec::Unsupported => shim::fail(Errno::NOTSUP),
    }
}

/// The compat-side timeouts of each socket, by descriptor. Every new socket descriptor
/// (`sock_open`, `sock_accept`, `sock_pair`) starts without, so a reused number does not
/// inherit them.
#[derive(Debug, Default)]
pub struct Timeouts(Mutex<HashMap<Fd, [Option<Duration>; 2]>>);

impl Timeouts {
    fn index(wait: Wait) -> usize {
        match wait {
            Wait::Connect => 0,
            Wait::Accept => 1,
        }
    }

    fn get(&self, fd: Fd, wait: Wait) -> Option<Duration> {
        let map = self.0.lock().unwrap();
        map.get(&fd).and_then(|t| t[Self::index(wait)])
    }

    fn set(&self, fd: Fd, wait: Wait, timeout: Option<Duration>) {
        let mut map = self.0.lock().unwrap();
        let entry = map.entry(fd).or_default();
        entry[Self::index(wait)] = timeout;
        if entry.iter().all(Option::is_none) {
            map.remove(&fd);
        }
    }

    /// A copy for a fork child.
    pub(crate) fn fork(&self) -> Self {
        Self(Mutex::new(self.0.lock().unwrap().clone()))
    }

    /// Forgets `fd`'s timeouts, for a descriptor that now names a new socket.
    pub(crate) fn forget(&self, fd: Fd) {
        self.0.lock().unwrap().remove(&fd);
    }
}

/// Forgets the timeouts of a descriptor that names a new socket now.
pub(crate) async fn new_socket<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<()> {
    frame.proc().await?.net.timeouts.forget(fd);
    Ok(())
}

async fn timeout<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    wait: Wait,
) -> V1Result<Option<Duration>> {
    Ok(frame.proc().await?.net.timeouts.get(fd, wait))
}

/// Whether `fd` is non-blocking, and its status flags.
async fn status_flags<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<OpenFlags> {
    let flags = frame
        .syscalls
        .fcntl64(&frame.caller(), fd, FcntlCmd::F_GETFL, 0)
        .await?;
    Ok(OpenFlags(flags as u32))
}

/// The `CONNECT_TIMEOUT` of a blocking socket.
pub(crate) async fn connect_timeout<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<Option<Duration>> {
    let Some(timeout) = timeout(frame, fd, Wait::Connect).await? else {
        return Ok(None);
    };
    let flags = status_flags(frame, fd).await?;
    Ok((!flags.contains(OpenFlags::O_NONBLOCK)).then_some(timeout))
}

/// A blocking `connect` that gives up with `TIMEDOUT` after `timeout`: the socket is made
/// non-blocking for the call, which then waits for the handshake itself. `again` says whether
/// this is a rerun after a signal, when the earlier attempt may have connected already.
pub(crate) async fn connect_within<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    addr: ConstSockaddrPtr,
    len: u32,
    timeout: Duration,
    again: bool,
) -> V1Result<()> {
    let flags = status_flags(frame, fd).await?;
    let caller = frame.caller();
    frame
        .syscalls
        .fcntl64(
            &caller,
            fd,
            FcntlCmd::F_SETFL,
            (flags | OpenFlags::O_NONBLOCK).0,
        )
        .await?;
    let result = async {
        use wasixv2_abi::Errno as L;
        match frame.syscalls.connect(&caller, fd, addr, len).await {
            Ok(_) => return Ok(()),
            Err(SysErr::Errno(L::EISCONN)) if again => return Ok(()),
            Err(SysErr::Errno(L::EINPROGRESS | L::EALREADY)) => {}
            Err(err) => return Err(err.into()),
        }
        if wait_fd(frame, fd, PollEvents::POLLOUT, Some(timeout))
            .await?
            .is_empty()
        {
            return shim::fail(Errno::TIMEDOUT);
        }
        match int_option(frame, fd, SocketLevel::SOL_SOCKET, SocketOption::SO_ERROR.0).await? {
            0 => Ok(()),
            errno => Err(wasixv2_abi::Errno(errno).into()),
        }
    }
    .await;
    // Restoring the flags never blocks, so it is safe after an interrupted wait.
    let restored = frame
        .syscalls
        .fcntl64(&caller, fd, FcntlCmd::F_SETFL, flags.0)
        .await;
    result?;
    restored?;
    Ok(())
}

/// Waits out the `ACCEPT_TIMEOUT` of a blocking listener: `TIMEDOUT` if no connection
/// arrives in time.
pub(crate) async fn wait_accept<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<()> {
    let Some(timeout) = timeout(frame, fd, Wait::Accept).await? else {
        return Ok(());
    };
    if status_flags(frame, fd)
        .await?
        .contains(OpenFlags::O_NONBLOCK)
    {
        return Ok(());
    }
    if wait_fd(frame, fd, PollEvents::POLLIN, Some(timeout))
        .await?
        .is_empty()
    {
        return shim::fail(Errno::TIMEDOUT);
    }
    Ok(())
}

/// `setsockopt` with an `int`.
async fn set_int<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    level: SocketLevel,
    name: i32,
    value: i32,
) -> V1Result<()> {
    set_bytes(frame, fd, level, name, &value.to_le_bytes()).await
}

async fn set_bytes<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    level: SocketLevel,
    name: i32,
    value: &[u8],
) -> V1Result<()> {
    let value = frame.mem.alloc_bytes(value)?;
    frame
        .syscalls
        .setsockopt(&frame.caller(), fd, level, name, value)
        .await?;
    Ok(())
}

/// `getsockopt` into `T`.
async fn get_value<S: Syscalls + Control + 'static, T: GuestLayout>(
    frame: &Frame<'_, S>,
    fd: Fd,
    level: SocketLevel,
    name: i32,
) -> V1Result<T> {
    let value = frame.mem.alloc_mut::<T>()?;
    let len = frame.mem.alloc(&(T::SIZE as u32))?;
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

pub(crate) async fn sock_set_opt_flag<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    flag: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| match target(option, Kind::Flag)? {
        Target::Linux(level, name) => {
            set_int(frame, Fd(fd), level, name, i32::from(flag as u8 != 0)).await
        }
        _ => Ok(()),
    })
    .await
}

pub(crate) async fn sock_get_opt_flag<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    ro_flag: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let on = match target(option, Kind::Flag)? {
            Target::Linux(level, name) => int_option(frame, Fd(fd), level, name).await? != 0,
            _ => false,
        };
        let flag = if on { Bool::TRUE } else { Bool::FALSE };
        frame.write(ro_flag as u32, &flag).await
    })
    .await
}

pub(crate) async fn sock_set_opt_size<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    size: i64,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| match target(option, Kind::Size)? {
        Target::Linux(level, name) => {
            let value = (size as u64).min(i32::MAX as u64) as i32;
            set_int(frame, Fd(fd), level, name, value).await
        }
        _ => Ok(()),
    })
    .await
}

pub(crate) async fn sock_get_opt_size<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    ro_size: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let Target::Linux(level, name) = target(option, Kind::Size)? else {
            return frame.write(ro_size as u32, &0u64).await;
        };
        let value = int_option(frame, Fd(fd), level, name).await?;
        let value = if name == SocketOption::SO_TYPE.0 && level == SocketLevel::SOL_SOCKET {
            u64::from(v1_socktype(value).0)
        } else if name == SocketOption::SO_ERROR.0 && level == SocketLevel::SOL_SOCKET {
            // A pending error, in v1 numbering.
            match value {
                0 => 0,
                errno => u64::from(Errno::from_linux(wasixv2_abi::Errno(errno)).0),
            }
        } else {
            value.max(0) as u64
        };
        frame.write(ro_size as u32, &value).await
    })
    .await
}

/// A Linux socket type as v1 numbers it.
fn v1_socktype(linux: i32) -> Socktype {
    match SocketType(linux) {
        SocketType::SOCK_STREAM => Socktype::STREAM,
        SocketType::SOCK_DGRAM => Socktype::DGRAM,
        SocketType::SOCK_RAW => Socktype::RAW,
        SocketType::SOCK_SEQPACKET => Socktype::SEQPACKET,
        _ => Socktype::UNUSED,
    }
}

/// An optional time from the guest; `INVAL` for a bad tag.
fn decode_time(time: OptionTimestamp) -> V1Result<Option<Duration>> {
    match time.tag {
        OptionTag::NONE => Ok(None),
        OptionTag::SOME => Ok(Some(Duration::from_nanos(time.timestamp))),
        _ => shim::fail(Errno::INVAL),
    }
}

fn encode_time(time: Option<Duration>) -> OptionTimestamp {
    match time {
        Some(time) => OptionTimestamp {
            tag: OptionTag::SOME,
            timestamp: u64::try_from(time.as_nanos()).unwrap_or(u64::MAX),
        },
        None => OptionTimestamp {
            tag: OptionTag::NONE,
            timestamp: 0,
        },
    }
}

pub(crate) async fn sock_set_opt_time<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    time: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let target = target(option, Kind::Time)?;
        let time = decode_time(frame.read(time as u32).await?)?;
        let fd = Fd(fd);
        match target {
            Target::Linux(level, name) => {
                // A zero timeval means no timeout, as `None` does.
                let time = time.unwrap_or_default();
                let tv = KernelSockTimeval {
                    tv_sec: time.as_secs().min(i64::MAX as u64) as i64,
                    tv_usec: i64::from(time.subsec_micros()),
                };
                let mut raw = [0u8; KernelSockTimeval::SIZE];
                tv.write_to(&mut raw);
                set_bytes(frame, fd, level, name, &raw).await
            }
            Target::Linger => {
                let linger = Linger {
                    l_onoff: i32::from(time.is_some()),
                    l_linger: time.map_or(0, |t| t.as_secs().min(i32::MAX as u64) as i32),
                };
                let mut raw = [0u8; Linger::SIZE];
                linger.write_to(&mut raw);
                set_bytes(
                    frame,
                    fd,
                    SocketLevel::SOL_SOCKET,
                    SocketOption::SO_LINGER.0,
                    &raw,
                )
                .await
            }
            Target::Compat(wait) => {
                // Only a socket can carry one, as under v1.
                int_option(frame, fd, SocketLevel::SOL_SOCKET, SocketOption::SO_TYPE.0).await?;
                frame.proc().await?.net.timeouts.set(fd, wait, time);
                Ok(())
            }
            Target::Noop => Ok(()),
        }
    })
    .await
}

pub(crate) async fn sock_get_opt_time<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    option: i32,
    ro_time: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let fd = Fd(fd);
        let time = match target(option, Kind::Time)? {
            Target::Linux(level, name) => {
                let tv: KernelSockTimeval = get_value(frame, fd, level, name).await?;
                let time = Duration::new(tv.tv_sec.max(0) as u64, 0)
                    + Duration::from_micros(tv.tv_usec.max(0) as u64);
                (!time.is_zero()).then_some(time)
            }
            Target::Linger => {
                let linger: Linger = get_value(
                    frame,
                    fd,
                    SocketLevel::SOL_SOCKET,
                    SocketOption::SO_LINGER.0,
                )
                .await?;
                (linger.l_onoff != 0).then(|| Duration::from_secs(linger.l_linger.max(0) as u64))
            }
            Target::Compat(wait) => {
                int_option(frame, fd, SocketLevel::SOL_SOCKET, SocketOption::SO_TYPE.0).await?;
                timeout(frame, fd, wait).await?
            }
            Target::Noop => None,
        };
        frame.write(ro_time as u32, &encode_time(time)).await
    })
    .await
}

/// `IP_ADD_MEMBERSHIP`/`IP_DROP_MEMBERSHIP` with a `struct ip_mreq`.
async fn multicast_v4<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
    name: IpOption,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let group: AddrIp4 = frame.read(group as u32).await?;
        let iface: AddrIp4 = frame.read(iface as u32).await?;
        let mut mreq = [0u8; 8];
        mreq[..4].copy_from_slice(&group);
        mreq[4..].copy_from_slice(&iface);
        set_bytes(frame, Fd(fd), SocketLevel::SOL_IP, name.0, &mreq).await
    })
    .await
}

/// `IPV6_ADD_MEMBERSHIP`/`IPV6_DROP_MEMBERSHIP` with a `struct ipv6_mreq`.
async fn multicast_v6<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
    name: Ipv6Option,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let group: AddrIp6 = frame.read(group as u32).await?;
        let mut mreq = [0u8; 20];
        mreq[..16].copy_from_slice(&group.octets);
        mreq[16..].copy_from_slice(&iface.to_le_bytes());
        set_bytes(frame, Fd(fd), SocketLevel::SOL_IPV6, name.0, &mreq).await
    })
    .await
}

pub(crate) async fn sock_join_multicast_v4<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
) -> Result<i32, RuntimeError> {
    multicast_v4(env, fd, group, iface, IpOption::IP_ADD_MEMBERSHIP).await
}

pub(crate) async fn sock_leave_multicast_v4<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
) -> Result<i32, RuntimeError> {
    multicast_v4(env, fd, group, iface, IpOption::IP_DROP_MEMBERSHIP).await
}

pub(crate) async fn sock_join_multicast_v6<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
) -> Result<i32, RuntimeError> {
    multicast_v6(env, fd, group, iface, Ipv6Option::IPV6_ADD_MEMBERSHIP).await
}

pub(crate) async fn sock_leave_multicast_v6<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    group: i32,
    iface: i32,
) -> Result<i32, RuntimeError> {
    multicast_v6(env, fd, group, iface, Ipv6Option::IPV6_DROP_MEMBERSHIP).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every v1 option has an arm, and only `PROMISCUOUS` is refused.
    #[test]
    fn every_option_is_mapped() {
        for option in 0..=Sockoption::MAX.0 {
            match spec(Sockoption(option)) {
                Spec::Option(..) => {}
                Spec::Unsupported => assert_eq!(Sockoption(option), Sockoption::PROMISCUOUS),
                Spec::Unknown => panic!("v1 option {option} has no mapping"),
            }
        }
        assert_eq!(spec(Sockoption(Sockoption::MAX.0 + 1)), Spec::Unknown);
    }

    #[test]
    fn kinds_must_match() {
        assert_eq!(
            target(Sockoption::REUSE_ADDR.0.into(), Kind::Flag),
            Ok(Target::Linux(
                SocketLevel::SOL_SOCKET,
                SocketOption::SO_REUSEADDR.0
            ))
        );
        assert!(target(Sockoption::REUSE_ADDR.0.into(), Kind::Size).is_err());
        assert_eq!(
            target(Sockoption::NOOP.0.into(), Kind::Time),
            Ok(Target::Noop)
        );
        assert_eq!(
            target(Sockoption::PROMISCUOUS.0.into(), Kind::Flag),
            Err(crate::errno::Error::V1(Errno::NOTSUP))
        );
    }

    #[test]
    fn times() {
        let none = encode_time(None);
        assert_eq!(decode_time(none), Ok(None));
        let some = encode_time(Some(Duration::from_millis(1500)));
        assert_eq!(some.timestamp, 1_500_000_000);
        assert_eq!(decode_time(some), Ok(Some(Duration::from_millis(1500))));
        let bad = OptionTimestamp {
            tag: OptionTag(7),
            timestamp: 0,
        };
        assert!(decode_time(bad).is_err());
    }

    #[test]
    fn socket_types_back_to_v1() {
        assert_eq!(v1_socktype(SocketType::SOCK_STREAM.0), Socktype::STREAM);
        assert_eq!(
            v1_socktype(SocketType::SOCK_SEQPACKET.0),
            Socktype::SEQPACKET
        );
    }
}
