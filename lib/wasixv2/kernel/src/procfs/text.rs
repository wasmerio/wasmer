//! The contents of the process filesystem's files, in the formats of `fs/proc` and
//! `Documentation/filesystems/proc.rst`.

use std::{fmt::Write as _, net::SocketAddr, sync::Arc, time::Duration};

use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{FsResult, MntNs, Mount, MountFlags, Path};
use wasixv2_vnet::SocketKind;

use super::{Source, node::Scope};
use crate::{
    file::OpenFile,
    mm::{Mm, PAGE_SIZE, RegionKind},
    ns::{NetNs, NsProxy},
    resource::{ADDRESS_SPACE, TICKS},
    sched::cpus,
    sync::MutexExt,
};

/// `PID_MAX_LIMIT` on 64-bit Linux; pids are never reused here, so any value below it works.
const PID_MAX: u32 = 4_194_304;
/// `fs.nr_open` and `fs.pipe-max-size`, Linux's defaults.
const NR_OPEN: u32 = 1 << 20;
const PIPE_MAX_SIZE: u32 = 1 << 20;

/// The system-wide files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Global {
    Cpuinfo,
    Meminfo,
    Uptime,
    Loadavg,
    Stat,
    Version,
    Filesystems,
    Cmdline,
    Hostname,
    Domainname,
    Ostype,
    Osrelease,
    PidMax,
    Overcommit,
    MmapMinAddr,
    FileMax,
    NrOpen,
    PipeMaxSize,
}

/// The files of a network namespace (`/proc/<pid>/net`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Net {
    /// `tcp`, or `tcp6`.
    Tcp(bool),
    Udp(bool),
    Unix,
    Dev,
    /// `route`, `ipv6_route` and `if_inet6`.
    Route,
    Ipv6Route,
    IfInet6,
}

impl Global {
    /// Tunables are writable by root; the rest are read-only.
    pub(super) fn mode(self) -> u32 {
        match self {
            Self::Hostname
            | Self::Domainname
            | Self::PidMax
            | Self::Overcommit
            | Self::MmapMinAddr
            | Self::FileMax
            | Self::NrOpen
            | Self::PipeMaxSize => 0o644,
            _ => 0o444,
        }
    }
}

/// The files of a process or thread directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PerPid {
    Cmdline,
    Comm,
    Environ,
    Limits,
    Maps,
    Mountinfo,
    Mounts,
    Stat,
    Statm,
    Status,
}

/// A system file as a reader in the namespaces `ns` sees it.
pub(super) fn global(source: &Source, ns: &NsProxy, file: Global) -> FsResult<Vec<u8>> {
    let line = |s: &str| format!("{s}\n").into_bytes();
    let names = *ns.uts.lock_or_recover();
    let uts = |field: &[u8; 65]| {
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..len]).into_owned()
    };
    Ok(match file {
        Global::Cpuinfo => cpuinfo(),
        Global::Meminfo => meminfo(source),
        Global::Uptime => {
            let up = source.host.monotonic();
            line(&format!("{} 0.00", centis(up)))
        }
        Global::Loadavg => {
            let (tasks, last) = source.task_counts();
            line(&format!("0.00 0.00 0.00 1/{tasks} {last}"))
        }
        Global::Stat => stat(source),
        Global::Version => line(&format!(
            "{} version {} {}",
            uts(&names.sysname),
            uts(&names.release),
            uts(&names.version)
        )),
        Global::Filesystems => filesystems(source),
        Global::Cmdline => line(""),
        Global::Hostname => line(&uts(&names.nodename)),
        Global::Domainname => line(&uts(&names.domainname)),
        Global::Ostype => line(&uts(&names.sysname)),
        Global::Osrelease => line(&uts(&names.release)),
        Global::PidMax => line(&PID_MAX.to_string()),
        // `OVERCOMMIT_GUESS`.
        Global::Overcommit => line("0"),
        Global::MmapMinAddr => line(&crate::mm::MMAP_MIN_ADDR.to_string()),
        Global::FileMax => line(&i64::MAX.to_string()),
        Global::NrOpen => line(&NR_OPEN.to_string()),
        Global::PipeMaxSize => line(&PIPE_MAX_SIZE.to_string()),
    })
}

/// A file of the network namespace `ns`.
pub(super) fn net(ns: &NetNs, file: Net) -> Vec<u8> {
    match file {
        Net::Tcp(v6) => sockets(ns, SocketKind::Stream, v6),
        Net::Udp(v6) => sockets(ns, SocketKind::Datagram, v6),
        Net::Unix => unix_sockets(ns),
        Net::Dev => net_dev(ns),
        Net::Route => net_route(ns),
        Net::Ipv6Route => net_ipv6_route(ns),
        Net::IfInet6 => net_if_inet6(ns),
    }
}

/// Seconds with two decimals, as `uptime` shows them.
fn centis(d: Duration) -> String {
    format!("{}.{:02}", d.as_secs(), d.subsec_millis() / 10)
}

fn ticks(d: Duration) -> u128 {
    d.as_nanos() * TICKS / 1_000_000_000
}

fn cpuinfo() -> Vec<u8> {
    let mut out = String::new();
    for cpu in 0..cpus() {
        let _ = write!(out, "processor\t: {cpu}\nmodel name\t: wasm32\n\n");
    }
    out.into_bytes()
}

/// Memory is the 4 GiB a wasm32 guest can address, less what the processes use, as `sysinfo`
/// counts it.
fn meminfo(source: &Source) -> Vec<u8> {
    let used: u64 = source
        .pids()
        .into_iter()
        .filter_map(|pid| source.mm(Scope { pid, tid: None }))
        .map(|mm| mm.size())
        .sum();
    let total = ADDRESS_SPACE / 1024;
    let free = (ADDRESS_SPACE.saturating_sub(used)) / 1024;
    let mut out = String::new();
    for (name, kb) in [
        ("MemTotal", total),
        ("MemFree", free),
        ("MemAvailable", free),
        ("Buffers", 0),
        ("Cached", 0),
        ("SwapCached", 0),
        ("Active", used / 1024),
        ("Inactive", 0),
        ("SwapTotal", 0),
        ("SwapFree", 0),
        ("Shmem", 0),
        ("AnonPages", used / 1024),
        ("Mapped", 0),
        ("CommitLimit", total),
        ("Committed_AS", used / 1024),
        ("VmallocTotal", 0),
        ("VmallocUsed", 0),
    ] {
        let _ = writeln!(out, "{:<16}{kb:>8} kB", format!("{name}:"));
    }
    out.into_bytes()
}

/// `show_stat`. The machine's line counts the user and system time of every process, the
/// reaped ones included, and the rest of the uptime as idle; the per-CPU lines, which no
/// accounting tells apart, are idle time only.
fn stat(source: &Source) -> Vec<u8> {
    let up = ticks(source.host.monotonic());
    let used = source.cpu_times();
    let (user, system) = (ticks(used.user), ticks(used.system));
    let idle = up.saturating_sub(user + system);
    let boot = source
        .host
        .realtime()
        .saturating_sub(source.host.monotonic())
        .as_secs();
    let (tasks, last) = source.task_counts();
    let mut out = String::new();
    let _ = writeln!(out, "cpu  {user} 0 {system} {idle} 0 0 0 0 0 0");
    for cpu in 0..cpus() {
        let _ = writeln!(out, "cpu{cpu} 0 0 0 {up} 0 0 0 0 0 0");
    }
    let _ = write!(
        out,
        "intr 0\nctxt 0\nbtime {boot}\nprocesses {last}\nprocs_running {}\nprocs_blocked 0\nsoftirq 0 0 0 0 0 0 0 0 0 0 0\n",
        tasks.min(1)
    );
    out.into_bytes()
}

/// The kernel's internal filesystem types, then those `mount` can make, none of which needs a
/// device (`filesystems_proc_show`).
fn filesystems(source: &Source) -> Vec<u8> {
    let mut names: Vec<String> = ["anon_inodefs", "pipefs", "sockfs"]
        .map(String::from)
        .into();
    names.extend(source.filesystems.names());
    let mut out = String::new();
    for name in names {
        let _ = writeln!(out, "nodev\t{name}");
    }
    out.into_bytes()
}

/// `tcp4_seq_show` and `udp4_seq_show` (and their v6 forms) over the port table.
fn sockets(ns: &NetNs, kind: SocketKind, v6: bool) -> Vec<u8> {
    let mut out = String::new();
    let addr_width = if v6 { 32 } else { 8 };
    let _ = writeln!(
        out,
        "  sl  {:<w$} {:<w$} st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode",
        "local_address",
        "rem_address",
        w = addr_width + 5
    );
    let ports = ns
        .network
        .as_ref()
        .map(|net| net.ports())
        .unwrap_or_default();
    let entries = ports
        .into_iter()
        .filter(|port| port.kind == kind && port.local.is_ipv6() == v6);
    for (sl, port) in entries.enumerate() {
        let peer = port.peer.unwrap_or_else(|| match port.local {
            SocketAddr::V4(_) => SocketAddr::from(([0, 0, 0, 0], 0)),
            SocketAddr::V6(_) => SocketAddr::from(([0u16; 8], 0)),
        });
        // TCP_LISTEN, TCP_ESTABLISHED, TCP_CLOSE.
        let state = match (port.listening, port.peer.is_some()) {
            (true, _) => 0x0a,
            (false, true) => 0x01,
            (false, false) => 0x07,
        };
        let _ = writeln!(
            out,
            "{sl:4}: {} {} {state:02X} 00000000:00000000 00:00000000 00000000 {:5}        0 0",
            hex_addr(port.local),
            hex_addr(peer),
            // TODO: the socket inode's owner (`sock_i_uid`); ports do not say whose they are.
            0,
        );
    }
    out.into_bytes()
}

/// An address as `/proc/net/tcp` prints it: the address's bytes as native 32-bit words in hex,
/// then the port.
fn hex_addr(addr: SocketAddr) -> String {
    let words = match addr {
        SocketAddr::V4(v4) => vec![u32::from_ne_bytes(v4.ip().octets())],
        SocketAddr::V6(v6) => v6
            .ip()
            .octets()
            .chunks(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    };
    let mut out: String = words.iter().map(|w| format!("{w:08X}")).collect();
    let _ = write!(out, ":{:04X}", addr.port());
    out
}

/// `unix_seq_show`: every Unix socket of the namespace.
fn unix_sockets(ns: &NetNs) -> Vec<u8> {
    /// `__SO_ACCEPTCON`, and `SS_UNCONNECTED` through `SS_DISCONNECTING`.
    const ACCEPTCON: u32 = 1 << 16;
    const UNCONNECTED: u8 = 1;
    const CONNECTING: u8 = 2;
    const CONNECTED: u8 = 3;
    const DISCONNECTING: u8 = 4;
    let mut out = b"Num       RefCount Protocol Flags    Type St Inode Path\n".to_vec();
    for sock in ns.unix_names.sockets() {
        // Less the reference held here.
        let entry = sock.seq_entry(Arc::strong_count(&sock) - 1);
        let flags = if entry.listening { ACCEPTCON } else { 0 };
        let state = match (entry.ino != 0, entry.established) {
            (true, true) => CONNECTED,
            (true, false) => UNCONNECTED,
            (false, true) => CONNECTING,
            (false, false) => DISCONNECTING,
        };
        let line = format!(
            "{:08}: {:08X} {:08X} {flags:08X} {:04X} {state:02X} {:5}",
            0, entry.refs, 0, entry.socket_type.0, entry.ino
        );
        out.extend(line.as_bytes());
        if let Some(addr) = entry.addr {
            out.push(b' ');
            let path = &addr[2..];
            match path.first() {
                Some(0) => {
                    out.push(b'@');
                    out.extend(path[1..].iter().map(|&b| if b == 0 { b'@' } else { b }));
                }
                _ => out.extend(path.iter().take(path.len().saturating_sub(1))),
            }
        }
        out.push(b'\n');
    }
    out
}

/// `dev_seq_show`: the interfaces, with no traffic counted.
fn net_dev(ns: &NetNs) -> Vec<u8> {
    let mut out = String::from(
        "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
    );
    let interfaces = ns
        .network
        .as_ref()
        .map(|net| net.interfaces())
        .unwrap_or_default();
    for interface in interfaces {
        let _ = writeln!(out, "{:>6}: {}", interface.name, ["       0"; 16].join(" "));
    }
    out.into_bytes()
}

/// `fib_route_seq_show`: the IPv4 routes of the main table, addresses as the big-endian words
/// they are in hex, every line padded to 127 columns.
fn net_route(ns: &NetNs) -> Vec<u8> {
    use std::net::IpAddr;
    use wasixv2_vnet::iface::rt::{RT_TABLE_MAIN, RTN_PROHIBIT, RTN_UNICAST, RTN_UNREACHABLE};
    const RTF_UP: u32 = 0x1;
    const RTF_GATEWAY: u32 = 0x2;
    const RTF_HOST: u32 = 0x4;
    const RTF_REJECT: u32 = 0x200;
    let (routes, interfaces) = ns
        .network
        .as_ref()
        .map(|n| (n.routes(), n.interfaces()))
        .unwrap_or_default();
    let word = |ip: Option<IpAddr>| match ip {
        Some(IpAddr::V4(v4)) => u32::from_le_bytes(v4.octets()),
        _ => 0,
    };
    let mut out = format!(
        "{:<127}\n",
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT"
    );
    for r in routes
        .iter()
        .filter(|r| r.dst.is_ipv4() && r.table == RT_TABLE_MAIN)
    {
        let reject = matches!(r.kind, RTN_UNREACHABLE | RTN_PROHIBIT);
        if r.kind != RTN_UNICAST && !reject {
            continue;
        }
        let dev = interfaces
            .iter()
            .find(|i| i.index == r.oif)
            .map_or("*", |i| i.name.as_str());
        let mask = u32::MAX
            .checked_shl(32 - u32::from(r.prefix_len))
            .unwrap_or(0);
        let mut flags = RTF_UP;
        if r.gateway.is_some() {
            flags |= RTF_GATEWAY;
        }
        if r.prefix_len == 32 {
            flags |= RTF_HOST;
        }
        if reject {
            flags |= RTF_REJECT;
        }
        let line = format!(
            "{dev}\t{:08X}\t{:08X}\t{flags:04X}\t0\t0\t{}\t{:08X}\t0\t0\t0",
            word(Some(r.dst)),
            word(r.gateway),
            r.metric,
            u32::from_le_bytes(mask.to_be_bytes()),
        );
        let _ = writeln!(out, "{line:<127}");
    }
    out.into_bytes()
}

/// `ipv6_route_native_seq_show`: every IPv6 route.
fn net_ipv6_route(ns: &NetNs) -> Vec<u8> {
    use std::net::{IpAddr, Ipv6Addr};
    use wasixv2_vnet::iface::rt::{RTN_PROHIBIT, RTN_UNICAST, RTN_UNREACHABLE};
    let (routes, interfaces) = ns
        .network
        .as_ref()
        .map(|n| (n.routes(), n.interfaces()))
        .unwrap_or_default();
    let hex = |ip: Option<IpAddr>| match ip {
        Some(IpAddr::V6(v6)) => format!("{:032x}", u128::from(v6)),
        _ => format!("{:032x}", u128::from(Ipv6Addr::UNSPECIFIED)),
    };
    let mut out = String::new();
    for r in routes.iter().filter(|r| r.dst.is_ipv6()) {
        let reject = matches!(r.kind, RTN_UNREACHABLE | RTN_PROHIBIT);
        if r.kind != RTN_UNICAST && !reject {
            continue;
        }
        let flags =
            0x1 | if r.gateway.is_some() { 0x2 } else { 0 } | if reject { 0x200 } else { 0 };
        let dev = interfaces
            .iter()
            .find(|i| i.index == r.oif)
            .map_or("lo", |i| i.name.as_str());
        let _ = writeln!(
            out,
            "{} {:02x} {} 00 {} {:08x} 00000000 00000000 {flags:08x} {dev:>8}",
            hex(Some(r.dst)),
            r.prefix_len,
            hex(None),
            hex(r.gateway),
            r.metric,
        );
    }
    out.into_bytes()
}

/// `if6_seq_show`: every IPv6 address, with `ipv6_addr_scope`'s scope bits.
fn net_if_inet6(ns: &NetNs) -> Vec<u8> {
    use std::net::IpAddr;
    let interfaces = ns
        .network
        .as_ref()
        .map(|n| n.interfaces())
        .unwrap_or_default();
    let mut out = String::new();
    for iface in &interfaces {
        for a in &iface.addresses {
            let IpAddr::V6(v6) = a.addr else {
                continue;
            };
            let scope = match v6 {
                v6 if v6.is_loopback() => 0x10,
                v6 if v6.is_unicast_link_local() => 0x20,
                v6 if v6.segments()[0] & 0xffc0 == 0xfec0 => 0x40,
                _ => 0x00,
            };
            let _ = writeln!(
                out,
                "{:032x} {:02x} {:02x} {scope:02x} {:02x} {:>8}",
                u128::from(v6),
                iface.index,
                a.prefix_len,
                a.flags & 0xff,
                iface.name,
            );
        }
    }
    out.into_bytes()
}

pub(super) fn per_pid(source: &Source, scope: Scope, file: PerPid) -> FsResult<Vec<u8>> {
    let info = source.process(scope.pid).ok_or(Errno::ESRCH)?;
    let process = &info.process;
    Ok(match file {
        PerPid::Cmdline => nul_joined(&process.args.lock_or_recover()),
        PerPid::Environ => nul_joined(&process.env.lock_or_recover()),
        PerPid::Comm => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            let mut comm = task.comm.name().to_vec();
            comm.push(b'\n');
            comm
        }
        PerPid::Limits => limits(process),
        PerPid::Maps => maps(source.mm(scope).as_deref()),
        PerPid::Mounts | PerPid::Mountinfo => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            let root = task.fs.lock_or_recover().root.clone();
            mounts(source, &task.ns.mnt.tree, &root, file == PerPid::Mountinfo)
        }
        PerPid::Stat => stat_line(source, scope, &info),
        PerPid::Statm => {
            let pages = source.mm(scope).map_or(0, |mm| mm.size()) / PAGE_SIZE;
            format!("{pages} {pages} 0 0 0 {pages} 0\n").into_bytes()
        }
        PerPid::Status => status(source, scope, &info),
    })
}

/// `cmdline` and `environ`: every string followed by a NUL.
fn nul_joined(strings: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in strings {
        out.extend_from_slice(s);
        out.push(0);
    }
    out
}

/// `proc_pid_limits`.
fn limits(process: &crate::Process) -> Vec<u8> {
    const NAMES: [(&str, &str); 16] = [
        ("Max cpu time", "seconds"),
        ("Max file size", "bytes"),
        ("Max data size", "bytes"),
        ("Max stack size", "bytes"),
        ("Max core file size", "bytes"),
        ("Max resident set", "bytes"),
        ("Max processes", "processes"),
        ("Max open files", "files"),
        ("Max locked memory", "bytes"),
        ("Max address space", "bytes"),
        ("Max file locks", "locks"),
        ("Max pending signals", "signals"),
        ("Max msgqueue size", "bytes"),
        ("Max nice priority", ""),
        ("Max realtime priority", ""),
        ("Max realtime timeout", "us"),
    ];
    let limits = *process.resources.lock_or_recover().limits();
    let value = |v: u64| match v {
        u64::MAX => "unlimited".to_string(),
        v => v.to_string(),
    };
    let mut out = format!(
        "{:<25} {:<20} {:<20} {:<10}\n",
        "Limit", "Soft Limit", "Hard Limit", "Units"
    );
    for ((name, unit), limit) in NAMES.iter().zip(limits) {
        let _ = writeln!(
            out,
            "{name:<25} {:<20} {:<20} {unit:<10}",
            value(limit.rlim_cur),
            value(limit.rlim_max)
        );
    }
    out.into_bytes()
}

/// `show_map_vma` for each region.
fn maps(mm: Option<&Mm>) -> Vec<u8> {
    let mut out = String::new();
    for region in mm.map(Mm::regions).unwrap_or_default() {
        let name = match region.kind {
            RegionKind::Image => "[image]",
            RegionKind::Heap => "[heap]",
            RegionKind::Anon => "",
        };
        let line = format!(
            "{:08x}-{:08x} rw-p 00000000 00:00 0",
            region.start, region.end
        );
        let _ = match name {
            "" => writeln!(out, "{line}"),
            name => writeln!(out, "{line:<73}{name}"),
        };
    }
    out.into_bytes()
}

/// `show_vfsmnt` and `show_mountinfo` for the mounts of the namespace `ns` that the process's
/// root reaches.
fn mounts(source: &Source, ns: &MntNs, root: &Path, info: bool) -> Vec<u8> {
    let Some(vfs) = source.vfs.upgrade() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in vfs.mount_info(ns, root) {
        let mount = &entry.mount;
        let point = vfs.d_path(&mount.root_path(), root);
        if point.starts_with(b"(unreachable)") {
            continue;
        }
        let sb = mount.sb();
        let devname = match mount.devname() {
            b"" => b"none".as_slice(),
            name => name,
        };
        if info {
            let _ = write!(
                out_string(&mut out),
                "{} {} {}:{} ",
                mount.id,
                entry.parent,
                sb.dev.major,
                sb.dev.minor
            );
            out.extend_from_slice(&escape(&root_in_fs(mount)));
            out.push(b' ');
            out.extend_from_slice(&escape(&point));
            let flags = mount.flags();
            let _ = write!(
                out_string(&mut out),
                " {}",
                mount_options(flags, flags.read_only)
            );
            // The tagged fields.
            let tags = [
                ("shared", entry.shared),
                ("master", entry.master),
                ("propagate_from", entry.propagate_from),
            ];
            for (tag, group) in tags {
                if let Some(group) = group {
                    let _ = write!(out_string(&mut out), " {tag}:{group}");
                }
            }
            if entry.unbindable {
                out.extend_from_slice(b" unbindable");
            }
            let _ = write!(out_string(&mut out), " - {} ", sb.info.name);
            out.extend_from_slice(&escape(devname));
            let fs_options = if sb.is_read_only() { "ro" } else { "rw" };
            let _ = write!(out_string(&mut out), " {fs_options}");
            sb.fs.show_options(&mut out);
            out.push(b'\n');
        } else {
            out.extend_from_slice(&escape(devname));
            out.push(b' ');
            out.extend_from_slice(&escape(&point));
            let options = mount_options(mount.flags(), mount.is_read_only());
            let _ = write!(out_string(&mut out), " {} {options}", sb.info.name);
            sb.fs.show_options(&mut out);
            out.extend_from_slice(b" 0 0\n");
        }
    }
    out
}

/// Appends formatted text to a byte buffer.
fn out_string(out: &mut Vec<u8>) -> impl std::fmt::Write + '_ {
    struct Bytes<'a>(&'a mut Vec<u8>);
    impl std::fmt::Write for Bytes<'_> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.extend_from_slice(s.as_bytes());
            Ok(())
        }
    }
    Bytes(out)
}

/// `show_mnt_opts` after `ro` or `rw` as `read_only` says: `/proc/<pid>/mounts` shows the
/// mount or its filesystem read-only (`__mnt_is_readonly`), `mountinfo` the mount's flag.
fn mount_options(flags: MountFlags, read_only: bool) -> String {
    let MountFlags {
        read_only: _,
        nosuid,
        nodev,
        noexec,
        noatime,
        nodiratime,
        strictatime,
        nosymfollow,
    } = flags;
    let mut options = vec![if read_only { "ro" } else { "rw" }];
    // `show_mnt_opts`'s order.
    for (set, name) in [
        (nosuid, "nosuid"),
        (nodev, "nodev"),
        (noexec, "noexec"),
        (noatime, "noatime"),
        (nodiratime, "nodiratime"),
        (!noatime && !strictatime, "relatime"),
        (nosymfollow, "nosymfollow"),
    ] {
        if set {
            options.push(name);
        }
    }
    options.join(",")
}

/// Where a mount's tree starts inside its filesystem, for `mountinfo`'s root field.
pub(crate) fn root_in_fs(mount: &Mount) -> Vec<u8> {
    let mut parts = Vec::new();
    let mut dentry = mount.root.clone();
    while let Some(parent) = dentry.parent() {
        parts.push(dentry.name());
        dentry = parent;
    }
    let mut out = Vec::new();
    for part in parts.iter().rev() {
        out.push(b'/');
        out.extend_from_slice(part);
    }
    if out.is_empty() {
        out.push(b'/');
    }
    out
}

/// `mangle`: space, tab, newline and backslash as octal escapes.
fn escape(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len());
    for &b in path {
        match b {
            b' ' | b'\t' | b'\n' | b'\\' => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            _ => out.push(b),
        }
    }
    out
}

/// The task's state letter: a zombie, stopped by a stop signal, or sleeping (a task reading
/// its own state is running, but a file does not know who reads it).
fn state(info: &super::ProcessInfo, task: Option<&super::TaskInfo>) -> (char, &'static str) {
    match info.zombie {
        true => ('Z', "zombie"),
        false if task.is_some_and(|task| task.signals.is_stopped()) => ('T', "stopped"),
        false => ('S', "sleeping"),
    }
}

/// `do_task_stat`.
fn stat_line(source: &Source, scope: Scope, info: &super::ProcessInfo) -> Vec<u8> {
    let process = &info.process;
    let pid = scope.tid.unwrap_or(scope.pid);
    let ident = process.ident.lock_or_recover().clone();
    let task = source.task(scope.pid, scope.tid);
    let comm = task
        .as_ref()
        .map_or(Vec::new(), |task| task.comm.name().to_vec());
    let comm = String::from_utf8_lossy(&comm).into_owned();
    // A thread's own time, or the group's with its dead threads'; `cutime` and `cstime` are
    // the group's reaped children's.
    let cpu = match scope.tid {
        Some(_) => task.as_ref().map(|task| task.cpu).unwrap_or_default(),
        None => process.cpu.times(),
    };
    let children = process.children.lock_or_recover().times;
    let sched = task.map(|task| task.sched).unwrap_or_default();
    let nice = sched.nice();
    let threads = source.threads(scope.pid).len().max(1);
    let start = ticks(process.start);
    let vsize = source.mm(scope).map_or(0, |mm| mm.size());
    let rss = vsize / PAGE_SIZE;
    let signals = source
        .task(scope.pid, scope.tid)
        .map(|task| task.signals.proc_status())
        .unwrap_or_default();
    let (letter, _) = state(info, source.task(scope.pid, scope.tid).as_ref());
    // `do_task_stat`: the controlling terminal's device and foreground group (0 for none).
    let (tty_nr, tpgid) = match &ident.tty {
        Some(tty) => {
            let device = tty.device();
            let tty_nr = crate::tty::encode_dev(device.major, device.minor);
            (i128::from(tty_nr), tty.pgrp().map_or(0, i128::from))
        }
        None => (0, -1),
    };
    let fields: Vec<String> = [
        info.parent as i128,               // ppid
        ident.pgid as i128,                // pgrp
        ident.sid as i128,                 // session
        tty_nr,                            // tty_nr
        tpgid,                             // tpgid
        0x0040_0000,                       // flags: PF_RANDOMIZE
        0,                                 // minflt
        0,                                 // cminflt
        0,                                 // majflt
        0,                                 // cmajflt
        ticks(cpu.user) as i128,           // utime
        ticks(cpu.system) as i128,         // stime
        ticks(children.user) as i128,      // cutime
        ticks(children.system) as i128,    // cstime
        sched.prio() as i128,              // priority
        nice as i128,                      // nice
        threads as i128,                   // num_threads
        0,                                 // itrealvalue
        start as i128,                     // starttime
        vsize as i128,                     // vsize
        rss as i128,                       // rss
        u64::MAX as i128,                  // rsslim
        0,                                 // startcode
        0,                                 // endcode
        0,                                 // startstack
        0,                                 // kstkesp
        0,                                 // kstkeip
        (signals[0] | signals[1]) as i128, // signal
        signals[2] as i128,                // blocked
        signals[3] as i128,                // sigignore
        signals[4] as i128,                // sigcatch
        0,                                 // wchan
        0,                                 // nswap
        0,                                 // cnswap
        info.exit_signal as i128,          // exit_signal
        0,                                 // processor
        sched.rt_priority() as i128,       // rt_priority
        sched.policy().0 as i128,          // policy
        0,                                 // delayacct_blkio_ticks
        0,                                 // guest_time
        0,                                 // cguest_time
        0,                                 // start_data
        0,                                 // end_data
        0,                                 // start_brk
        0,                                 // arg_start
        0,                                 // arg_end
        0,                                 // env_start
        0,                                 // env_end
        0,                                 // exit_code
    ]
    .iter()
    .map(i128::to_string)
    .collect();
    format!("{pid} ({comm}) {letter} {}\n", fields.join(" ")).into_bytes()
}

/// `proc_pid_status`.
fn status(source: &Source, scope: Scope, info: &super::ProcessInfo) -> Vec<u8> {
    let process = &info.process;
    let ident = process.ident.lock_or_recover().clone();
    let task = source.task(scope.pid, scope.tid);
    let umask = task
        .as_ref()
        .map_or(0o022, |task| task.fs.lock_or_recover().umask);
    let fd_size = task
        .as_ref()
        .map_or(0, |task| task.files.lock_or_recover().max_fds());
    let signals = task
        .as_ref()
        .map(|task| task.signals.proc_status())
        .unwrap_or_default();
    let pid = scope.tid.unwrap_or(scope.pid);
    let kb = task.as_ref().map_or(0, |task| task.mm.size()) / 1024;
    let locked_kb = task.as_ref().map_or(0, |task| task.mm.locked()) / 1024;
    let threads = source.threads(scope.pid).len().max(1);
    let (letter, state_name) = state(info, task.as_ref());
    let cred = process.cred();
    let (user, group) = (&cred.user, &cred.group);
    let comm = task
        .as_ref()
        .map_or(Vec::new(), |task| task.comm.name().to_vec());
    let affinity = task
        .as_ref()
        .map_or(Vec::new(), |task| task.sched.affinity().to_vec());
    let groups: String = cred.groups.iter().map(|gid| format!("{gid} ")).collect();
    let cpus = cpus();
    let mut out = String::new();
    let _ = write!(
        out,
        "Name:\t{}\nUmask:\t{umask:04o}\nState:\t{letter} ({state_name})\nTgid:\t{}\nNgid:\t0\nPid:\t{pid}\nPPid:\t{}\nTracerPid:\t0\n\
         Uid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\nFDSize:\t{fd_size}\nGroups:\t{groups}\n\
         NStgid:\t{}\nNSpid:\t{pid}\nNSpgid:\t{}\nNSsid:\t{}\n\
         VmPeak:\t{kb:8} kB\nVmSize:\t{kb:8} kB\nVmLck:\t{locked_kb:8} kB\nVmPin:\t       0 kB\nVmHWM:\t{kb:8} kB\nVmRSS:\t{kb:8} kB\n\
         RssAnon:\t{kb:8} kB\nRssFile:\t       0 kB\nRssShmem:\t       0 kB\nVmData:\t{kb:8} kB\nVmStk:\t       0 kB\nVmExe:\t       0 kB\nVmLib:\t       0 kB\nVmPTE:\t       0 kB\nVmSwap:\t       0 kB\n\
         Threads:\t{threads}\nSigQ:\t0/4096\nSigPnd:\t{:016x}\nShdPnd:\t{:016x}\nSigBlk:\t{:016x}\nSigIgn:\t{:016x}\nSigCgt:\t{:016x}\n\
         CapInh:\t{:016x}\nCapPrm:\t{:016x}\nCapEff:\t{:016x}\nCapBnd:\t{:016x}\nCapAmb:\t{:016x}\n\
         NoNewPrivs:\t{}\nSeccomp:\t0\nCpus_allowed:\t{}\nCpus_allowed_list:\t{}\n\
         voluntary_ctxt_switches:\t0\nnonvoluntary_ctxt_switches:\t0\n",
        String::from_utf8_lossy(&comm),
        scope.pid,
        info.parent,
        user.real,
        user.effective,
        user.saved,
        user.fs,
        group.real,
        group.effective,
        group.saved,
        group.fs,
        scope.pid,
        ident.pgid,
        ident.sid,
        signals[0],
        signals[1],
        signals[2],
        signals[3],
        signals[4],
        cred.inheritable,
        cred.permitted,
        cred.effective,
        cred.bounding,
        cred.ambient,
        u8::from(ident.no_new_privs),
        cpu_mask(&affinity, cpus),
        cpu_list(&affinity),
    );
    out.into_bytes()
}

/// A mask of `nbits` CPUs as `%*pb` prints it: 32-bit groups in hex, the highest first and only
/// as wide as its bits, separated by commas.
fn cpu_mask(words: &[u32], nbits: u32) -> String {
    let chunks = nbits.div_ceil(32).max(1) as usize;
    let top = (nbits as usize - (chunks - 1) * 32).div_ceil(4).max(1);
    (0..chunks)
        .rev()
        .map(|i| {
            let word = words.get(i).copied().unwrap_or(0);
            let width = if i == chunks - 1 { top } else { 8 };
            format!("{word:0width$x}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// A CPU mask as `%*pbl` prints it: ranges of set CPUs, such as `0-3,5`.
fn cpu_list(words: &[u32]) -> String {
    let set = |cpu: usize| words[cpu / 32] & (1 << (cpu % 32)) != 0;
    let bits = words.len() * 32;
    let mut ranges = Vec::new();
    let mut cpu = 0;
    while cpu < bits {
        if !set(cpu) {
            cpu += 1;
            continue;
        }
        let start = cpu;
        while cpu < bits && set(cpu) {
            cpu += 1;
        }
        ranges.push(match cpu - 1 {
            end if end == start => start.to_string(),
            end => format!("{start}-{end}"),
        });
    }
    ranges.join(",")
}

/// `seq_fdinfo`: position, flags (with `O_CLOEXEC` for a close-on-exec descriptor), mount and
/// inode.
pub(super) fn fdinfo(file: &OpenFile, cloexec: bool) -> Vec<u8> {
    let mut flags = file.flags().0;
    if cloexec {
        flags |= OpenFlags::O_CLOEXEC.0;
    }
    let path = file.path();
    format!(
        "pos:\t{}\nflags:\t0{flags:o}\nmnt_id:\t{}\nino:\t{}\n",
        file.pos(),
        path.mnt.id,
        path.dentry.node().ino()
    )
    .into_bytes()
}
