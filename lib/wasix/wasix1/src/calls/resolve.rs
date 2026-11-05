//! `resolve`: v1's name lookup syscall, which v2 dropped. The compat layer answers it as a
//! Linux libc would, through the guest's own files and sockets: numeric addresses, then
//! `/etc/hosts`, then DNS (A and AAAA over UDP, TCP when an answer is truncated) to the
//! `/etc/resolv.conf` name servers. Everything goes through `Syscalls`, so the guest's network
//! namespace, the policy layer and tracing all apply.
//!
//! wasix-libc only calls it for names that are neither numeric nor in its own hosts lookup.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use wasixv2_abi::{
    AtFd, Fd, GuestPtr, GuestPtrMut, MsgFlags, OpenFlags, PollEvents, SocketFlags, SocketType,
};
use wasixv2_syscalls::{Control, SysErr, Syscalls, UserMemory, UserSliceMut};
use wasmer::RuntimeError;

use crate::{
    abi::{Errno, GuestLayout, sock::AddrIp, sock::SockAddr},
    errno::V1Result,
    shim::{self, Env, Frame},
};

use super::{poll::wait_fd, sock};

/// The longest file the resolver reads (`/etc/hosts`, `/etc/resolv.conf`).
const FILE_LIMIT: u32 = 256 * 1024;
const DNS_PORT: u16 = 53;
/// The largest UDP answer accepted (EDNS is not requested, so servers send at most 512).
const UDP_ROOM: u32 = 4096;
const TCP_ROOM: u32 = 65535 + 2;

/// `resolv.conf`, as far as the resolver uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Config {
    nameservers: Vec<IpAddr>,
    timeout: Duration,
    attempts: u32,
}

impl Config {
    /// glibc's and musl's limits and defaults: at most three servers, 5 s per try, 2 tries,
    /// and the local host when no server is named.
    fn parse(text: &str) -> Self {
        let mut config = Self {
            nameservers: Vec::new(),
            timeout: Duration::from_secs(5),
            attempts: 2,
        };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("nameserver") => {
                    if let Some(ip) = words.next().and_then(|w| w.parse().ok())
                        && config.nameservers.len() < 3
                    {
                        config.nameservers.push(ip);
                    }
                }
                Some("options") => {
                    for option in words {
                        if let Some(n) = option
                            .strip_prefix("timeout:")
                            .and_then(|n| n.parse::<u64>().ok())
                        {
                            config.timeout = Duration::from_secs(n.clamp(1, 30));
                        } else if let Some(n) = option
                            .strip_prefix("attempts:")
                            .and_then(|n| n.parse::<u32>().ok())
                        {
                            config.attempts = n.clamp(1, 5);
                        }
                    }
                }
                _ => {}
            }
        }
        if config.nameservers.is_empty() {
            config.nameservers.push(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        config
    }
}

/// The addresses `/etc/hosts` lists for `name`, in file order.
fn hosts_lookup(text: &str, name: &str) -> Vec<IpAddr> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        let Some(ip) = words.next().and_then(|w| w.parse::<IpAddr>().ok()) else {
            continue;
        };
        if words.any(|alias| alias.eq_ignore_ascii_case(name)) && !found.contains(&ip) {
            found.push(ip);
        }
    }
    found
}

/// The DNS wire format, as much as an A/AAAA stub resolver needs (RFC 1035).
mod dns {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    pub const TYPE_A: u16 = 1;
    pub const TYPE_AAAA: u16 = 28;
    const CLASS_IN: u16 = 1;
    const FLAG_QR: u16 = 0x8000;
    const FLAG_TC: u16 = 0x0200;
    const FLAG_RD: u16 = 0x0100;
    pub const RCODE_NXDOMAIN: u16 = 3;

    /// A query for `name`'s `qtype` records, or `None` for a name DNS cannot carry.
    pub fn query(id: u16, name: &str, qtype: u16) -> Option<Vec<u8>> {
        let name = name.strip_suffix('.').unwrap_or(name);
        if name.is_empty() || name.len() > 253 {
            return None;
        }
        let mut msg = Vec::with_capacity(18 + name.len());
        msg.extend_from_slice(&id.to_be_bytes());
        msg.extend_from_slice(&FLAG_RD.to_be_bytes());
        msg.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        for label in name.split('.') {
            if label.is_empty() || label.len() > 63 {
                return None;
            }
            msg.push(label.len() as u8);
            msg.extend_from_slice(label.as_bytes());
        }
        msg.push(0);
        msg.extend_from_slice(&qtype.to_be_bytes());
        msg.extend_from_slice(&CLASS_IN.to_be_bytes());
        Some(msg)
    }

    /// A parsed answer.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Answer {
        pub id: u16,
        pub truncated: bool,
        pub rcode: u16,
        pub addrs: Vec<IpAddr>,
    }

    fn u16_at(msg: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]))
    }

    /// The offset after the (possibly compressed) name at `at`.
    fn skip_name(msg: &[u8], mut at: usize) -> Option<usize> {
        loop {
            let len = *msg.get(at)?;
            match len {
                0 => return Some(at + 1),
                l if l & 0xc0 == 0xc0 => return Some(at + 2),
                l if l & 0xc0 == 0 => at += 1 + l as usize,
                _ => return None,
            }
        }
    }

    /// The A and AAAA records of a response; `None` if it is not one.
    pub fn parse(msg: &[u8]) -> Option<Answer> {
        let id = u16_at(msg, 0)?;
        let flags = u16_at(msg, 2)?;
        if flags & FLAG_QR == 0 {
            return None;
        }
        let questions = u16_at(msg, 4)?;
        let answers = u16_at(msg, 6)?;
        let mut at = 12;
        for _ in 0..questions {
            at = skip_name(msg, at)? + 4;
        }
        let mut addrs = Vec::new();
        for _ in 0..answers {
            at = skip_name(msg, at)?;
            let rtype = u16_at(msg, at)?;
            let class = u16_at(msg, at + 2)?;
            let len = u16_at(msg, at + 8)? as usize;
            let data = msg.get(at + 10..at + 10 + len)?;
            at += 10 + len;
            if class != CLASS_IN {
                continue;
            }
            match (rtype, len) {
                (TYPE_A, 4) => {
                    addrs.push(IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(data).ok()?)))
                }
                (TYPE_AAAA, 16) => {
                    addrs.push(IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(data).ok()?)))
                }
                _ => {}
            }
        }
        Some(Answer {
            id,
            truncated: flags & FLAG_TC != 0,
            rcode: flags & 0xf,
            addrs,
        })
    }
}

/// What asking one server gave.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Found(Vec<IpAddr>),
    /// The name does not exist (or has no address); other servers would say the same.
    NotFound,
    /// No usable answer from this server.
    Failed,
}

/// The guest's monotonic clock in nanoseconds; deadlines are on it.
async fn monotonic<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>) -> V1Result<u64> {
    super::poll::now(frame, wasixv2_abi::ClockId::CLOCK_MONOTONIC).await
}

/// What is left until `deadline`.
async fn remaining<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    deadline: u64,
) -> V1Result<Duration> {
    Ok(Duration::from_nanos(
        deadline.saturating_sub(monotonic(frame).await?),
    ))
}

/// Reads the whole file at `path`; `None` if it cannot be read.
async fn read_file<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    path: &[u8],
) -> V1Result<Option<Vec<u8>>> {
    let caller = frame.caller();
    let cpath = frame.mem.alloc_cstr(path)?;
    let flags = OpenFlags::O_RDONLY | OpenFlags::O_CLOEXEC;
    let fd = match frame
        .syscalls
        .openat(&caller, Fd(AtFd::AT_FDCWD.0), cpath, flags, 0)
        .await
    {
        Ok(fd) => fd,
        Err(SysErr::Errno(_)) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let buf = frame.mem.alloc_zeroed(FILE_LIMIT)?;
    let mut len = 0u32;
    let result = loop {
        let rest = UserSliceMut::new(buf.addr + len, FILE_LIMIT - len);
        match frame.syscalls.read(&caller, fd, rest).await {
            Ok(0) => break Ok(()),
            Ok(n) => {
                len += n;
                if len == FILE_LIMIT {
                    break Ok(());
                }
            }
            Err(err) => break Err(err),
        }
    };
    // Closing never blocks, so it is safe after an interrupted read.
    let _ = frame.syscalls.close(&caller, fd).await;
    result?;
    Ok(Some(frame.mem.take_bytes(buf, len as usize)))
}

/// Sends `query` over TCP to `server` and returns the reply.
async fn query_tcp<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    server: SocketAddr,
    query: &[u8],
    deadline: u64,
) -> V1Result<Option<Vec<u8>>> {
    let caller = frame.caller();
    let family = match server {
        SocketAddr::V4(_) => wasixv2_abi::AddressFamily::AF_INET,
        SocketAddr::V6(_) => wasixv2_abi::AddressFamily::AF_INET6,
    };
    let ty = SocketType::SOCK_STREAM.0
        | (SocketFlags::SOCK_CLOEXEC | SocketFlags::SOCK_NONBLOCK).0 as i32;
    let fd = frame.syscalls.socket(&caller, family, ty, 0).await?;
    let result = async {
        let addr = frame
            .mem
            .alloc_bytes(&sock::to_linux(&SockAddr::Inet(server))?)?;
        match frame
            .syscalls
            .connect(&caller, fd, GuestPtr::new(addr.addr), addr.len)
            .await
        {
            Ok(_) | Err(SysErr::Errno(wasixv2_abi::Errno::EINPROGRESS)) => {}
            Err(SysErr::Errno(_)) => return Ok(None),
            Err(err) => return Err(err.into()),
        }
        let mut framed = (query.len() as u16).to_be_bytes().to_vec();
        framed.extend_from_slice(query);
        let mut sent = 0;
        while sent < framed.len() {
            let left = remaining(frame, deadline).await?;
            if wait_fd(frame, fd, PollEvents::POLLOUT, Some(left))
                .await?
                .is_empty()
            {
                return Ok(None);
            }
            let chunk = frame.mem.alloc_bytes(&framed[sent..])?;
            match frame.syscalls.write(&caller, fd, chunk).await {
                Ok(n) => sent += n as usize,
                Err(SysErr::Errno(wasixv2_abi::Errno::EAGAIN)) => {}
                Err(SysErr::Errno(_)) => return Ok(None),
                Err(err) => return Err(err.into()),
            }
        }
        let buf = frame.mem.alloc_zeroed(TCP_ROOM)?;
        let mut got = 0u32;
        loop {
            let have = frame.mem.take_bytes(buf, got as usize);
            if have.len() >= 2 {
                let want = u16::from_be_bytes([have[0], have[1]]) as usize;
                if have.len() >= 2 + want {
                    return Ok(Some(have[2..2 + want].to_vec()));
                }
            }
            let left = remaining(frame, deadline).await?;
            if wait_fd(frame, fd, PollEvents::POLLIN, Some(left))
                .await?
                .is_empty()
            {
                return Ok(None);
            }
            let rest = UserSliceMut::new(buf.addr + got, TCP_ROOM - got);
            match frame.syscalls.read(&caller, fd, rest).await {
                Ok(0) => return Ok(None),
                Ok(n) => got += n,
                Err(SysErr::Errno(wasixv2_abi::Errno::EAGAIN)) => {}
                Err(SysErr::Errno(_)) => return Ok(None),
                Err(err) => return Err(err.into()),
            }
        }
    }
    .await;
    let _ = frame.syscalls.close(&caller, fd).await;
    result
}

/// Asks `server` for `name`'s A and AAAA records over UDP within `timeout`, falling back to
/// TCP for a truncated answer.
async fn ask<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    server: IpAddr,
    name: &str,
    timeout: Duration,
) -> V1Result<Outcome> {
    let caller = frame.caller();
    let server = SocketAddr::new(server, DNS_PORT);
    let family = match server {
        SocketAddr::V4(_) => wasixv2_abi::AddressFamily::AF_INET,
        SocketAddr::V6(_) => wasixv2_abi::AddressFamily::AF_INET6,
    };
    // Query ids only need to tell the two queries apart and reject stray datagrams.
    let mut seed = [0u8; 2];
    let seed_buf = frame.mem.alloc_zeroed(2)?;
    frame
        .syscalls
        .getrandom(&caller, seed_buf, wasixv2_abi::GrndFlags(0))
        .await?;
    seed.copy_from_slice(&frame.mem.take_bytes(seed_buf, 2));
    let base = u16::from_le_bytes(seed) & !1;
    let mut queries = Vec::new();
    for (i, qtype) in [dns::TYPE_A, dns::TYPE_AAAA].into_iter().enumerate() {
        let Some(query) = dns::query(base + i as u16, name, qtype) else {
            return Ok(Outcome::NotFound);
        };
        queries.push(query);
    }
    let ty = SocketType::SOCK_DGRAM.0
        | (SocketFlags::SOCK_CLOEXEC | SocketFlags::SOCK_NONBLOCK).0 as i32;
    let fd = frame.syscalls.socket(&caller, family, ty, 0).await?;
    let deadline = monotonic(frame)
        .await?
        .saturating_add(timeout.as_nanos() as u64);
    let result = async {
        let addr = frame
            .mem
            .alloc_bytes(&sock::to_linux(&SockAddr::Inet(server))?)?;
        match frame
            .syscalls
            .connect(&caller, fd, GuestPtr::new(addr.addr), addr.len)
            .await
        {
            Ok(_) => {}
            Err(SysErr::Errno(_)) => return Ok(Outcome::Failed),
            Err(err) => return Err(err.into()),
        }
        for query in &queries {
            let data = frame.mem.alloc_bytes(query)?;
            match frame.syscalls.write(&caller, fd, data).await {
                Ok(_) => {}
                Err(SysErr::Errno(_)) => return Ok(Outcome::Failed),
                Err(err) => return Err(err.into()),
            }
        }
        let mut answers: [Option<dns::Answer>; 2] = [None, None];
        let buf = frame.mem.alloc_zeroed(UDP_ROOM)?;
        while answers.iter().any(Option::is_none) {
            let left = remaining(frame, deadline).await?;
            if wait_fd(frame, fd, PollEvents::POLLIN, Some(left))
                .await?
                .is_empty()
            {
                break;
            }
            let n = match frame
                .syscalls
                .recvfrom(
                    &caller,
                    fd,
                    buf,
                    MsgFlags(0),
                    GuestPtrMut::NULL,
                    GuestPtrMut::NULL,
                )
                .await
            {
                Ok(n) => n,
                Err(SysErr::Errno(wasixv2_abi::Errno::EAGAIN)) => continue,
                // Nobody listens there (`ECONNREFUSED`), or the route is gone.
                Err(SysErr::Errno(_)) => return Ok(Outcome::Failed),
                Err(err) => return Err(err.into()),
            };
            let reply = frame.mem.take_bytes(buf, n as usize);
            let Some(answer) = dns::parse(&reply) else {
                continue;
            };
            let Some(slot) = answer.id.checked_sub(base).filter(|&i| i < 2) else {
                continue;
            };
            let slot = slot as usize;
            if answer.truncated {
                let full = query_tcp(frame, server, &queries[slot], deadline).await?;
                answers[slot] = full.as_deref().and_then(dns::parse);
                if answers[slot].is_none() {
                    return Ok(Outcome::Failed);
                }
            } else {
                answers[slot] = Some(answer);
            }
        }
        let mut addrs = Vec::new();
        let mut not_found = false;
        let mut any = false;
        for answer in answers.iter().flatten() {
            any = true;
            match answer.rcode {
                0 => addrs.extend(answer.addrs.iter().copied()),
                dns::RCODE_NXDOMAIN => not_found = true,
                _ => {}
            }
        }
        // A records first, as they are asked for first.
        addrs.sort_by_key(|ip| ip.is_ipv6());
        Ok(match (addrs.is_empty(), any) {
            (false, _) => Outcome::Found(addrs),
            (true, true) if not_found || answers.iter().all(Option::is_some) => Outcome::NotFound,
            _ => Outcome::Failed,
        })
    }
    .await;
    // Closing never blocks, so it is safe after an interrupted wait.
    let _ = frame.syscalls.close(&caller, fd).await;
    result
}

/// The addresses of `name`.
async fn lookup<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    name: &str,
) -> V1Result<Vec<IpAddr>> {
    if let Ok(ip) = name.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    if let Some(hosts) = read_file(frame, b"/etc/hosts").await? {
        let found = hosts_lookup(&String::from_utf8_lossy(&hosts), name);
        if !found.is_empty() {
            return Ok(found);
        }
    }
    let conf = read_file(frame, b"/etc/resolv.conf")
        .await?
        .unwrap_or_default();
    let config = Config::parse(&String::from_utf8_lossy(&conf));
    for _ in 0..config.attempts {
        for &server in &config.nameservers {
            match ask(frame, server, name, config.timeout).await? {
                Outcome::Found(addrs) => return Ok(addrs),
                Outcome::NotFound => return shim::fail(Errno::NOENT),
                Outcome::Failed => {}
            }
        }
    }
    shim::fail(Errno::TIMEDOUT)
}

/// `resolve(host, host_len, port, addrs, naddrs, ro_naddrs)`: up to `naddrs` addresses of
/// `host` as [`AddrIp`]s. The port is only a hint and unused.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve<S: Syscalls + Control + 'static>(
    env: Env<S>,
    host: i32,
    host_len: i32,
    _port: i32,
    addrs: i32,
    naddrs: i32,
    ro_naddrs: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let mut name = vec![0; host_len as u32 as usize];
        frame.mem.guest().read(host as u32, &mut name).await?;
        while name.last() == Some(&0) {
            name.pop();
        }
        let Ok(name) = String::from_utf8(name) else {
            return shim::fail(Errno::INVAL);
        };
        let found = lookup(frame, &name).await?;
        let n = found.len().min(naddrs as u32 as usize);
        for (i, ip) in found.iter().take(n).enumerate() {
            let at = (addrs as u32).wrapping_add((i * AddrIp::SIZE) as u32);
            frame.write(at, &AddrIp::new(*ip)).await?;
        }
        frame.write(ro_naddrs as u32, &(n as u32)).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolv_conf() {
        let config = Config::parse(
            "# comment\nnameserver 10.0.0.1\nnameserver ::1\nsearch example\n\
             options ndots:1 timeout:2 attempts:9\nnameserver 1.1.1.1\nnameserver 8.8.8.8\n",
        );
        assert_eq!(
            config.nameservers,
            [
                "10.0.0.1".parse::<IpAddr>().unwrap(),
                "::1".parse().unwrap(),
                "1.1.1.1".parse().unwrap()
            ]
        );
        assert_eq!(config.timeout, Duration::from_secs(2));
        assert_eq!(config.attempts, 5);
        assert_eq!(
            Config::parse("").nameservers,
            [IpAddr::V4(Ipv4Addr::LOCALHOST)]
        );
    }

    #[test]
    fn hosts() {
        let text = "127.0.0.1\tlocalhost # loop\n::1 localhost ip6-localhost\n\
                    # 10.0.0.1 localhost\nbogus line\n10.1.1.1 db DB.local\n";
        assert_eq!(
            hosts_lookup(text, "localhost"),
            [
                "127.0.0.1".parse::<IpAddr>().unwrap(),
                "::1".parse().unwrap()
            ]
        );
        assert_eq!(
            hosts_lookup(text, "db.LOCAL"),
            ["10.1.1.1".parse::<IpAddr>().unwrap()]
        );
        assert!(hosts_lookup(text, "missing").is_empty());
    }

    #[test]
    fn dns_query_encoding() {
        let q = dns::query(0x1234, "a.bc.", dns::TYPE_AAAA).unwrap();
        assert_eq!(
            q,
            [
                0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 2, b'b', b'c', 0, 0, 28,
                0, 1
            ]
        );
        assert!(dns::query(1, "", dns::TYPE_A).is_none());
        assert!(dns::query(1, "a..b", dns::TYPE_A).is_none());
        assert!(dns::query(1, &"x".repeat(64), dns::TYPE_A).is_none());
    }

    #[test]
    fn dns_answer_parsing() {
        // The query for example.com A, answered with a CNAME and an A record (the owner
        // names compressed), and a truncated header.
        let mut msg = dns::query(7, "example.com", dns::TYPE_A).unwrap();
        msg[2] = 0x81;
        msg[3] = 0x80;
        msg[7] = 2;
        msg.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xc0, 12]);
        msg.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        let answer = dns::parse(&msg).unwrap();
        assert_eq!(
            answer,
            dns::Answer {
                id: 7,
                truncated: false,
                rcode: 0,
                addrs: vec!["93.184.216.34".parse().unwrap()],
            }
        );
        msg[2] |= 0x02;
        msg[3] |= 3;
        let answer = dns::parse(&msg).unwrap();
        assert!(answer.truncated);
        assert_eq!(answer.rcode, dns::RCODE_NXDOMAIN);
        // A query is not an answer; a cut-off answer is rejected.
        assert!(dns::parse(&dns::query(7, "example.com", dns::TYPE_A).unwrap()).is_none());
        assert!(dns::parse(&msg[..msg.len() - 2]).is_none());
    }
}
