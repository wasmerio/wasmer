//! The contents of the process filesystem's files, in the formats of `fs/proc` and
//! `Documentation/filesystems/proc.rst`.

use std::{
    fmt::Write as _,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{FsResult, MntNs, Mount, MountFlags, Path};
use wasixv2_vnet::{Domain, IcmpEntry, SocketKind};

use super::{Source, introspect, node::Scope};
use crate::{
    file::OpenFile,
    mm::{Mm, PAGE_SIZE, RegionKind},
    ns::{NetNs, NsProxy, PidNs, TimeNs, UserNs},
    resource::{ADDRESS_SPACE, TICKS},
    sched::cpus,
    sync::MutexExt,
    time::timens_add,
    uidgid::OVERFLOW_ID,
    userns::MapKind,
};

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
    NsLastPid,
    /// `vm.overcommit_memory`, `vm.overcommit_ratio`, `vm.overcommit_kbytes`.
    Overcommit,
    OvercommitRatio,
    OvercommitKbytes,
    /// `vm.max_map_count`.
    MaxMapCount,
    MmapMinAddr,
    FileMax,
    NrOpen,
    PipeMaxSize,
    DirNotifyEnable,
    LeasesEnable,
    LeaseBreakTime,
    /// `fs.aio-nr`, `fs.aio-max-nr`.
    AioNr,
    AioMaxNr,
    /// `kernel.overflowuid`, `kernel.overflowgid`.
    OverflowUid,
    OverflowGid,
    /// `kernel.cap_last_cap`.
    CapLastCap,
    /// `kernel.tainted`.
    Tainted,
    /// `kernel.yama.ptrace_scope`.
    PtraceScope,
    /// `/proc/keys`, `/proc/key-users`, which the node renders for its opener.
    Keys,
    KeyUsers,
    /// `user.max_*_namespaces` and `user.max_inotify_*` of the reader's user namespace.
    Ucount(crate::userns::UcountKind),
    /// `fs.inotify.max_user_instances` and `max_user_watches`: the initial namespace's.
    InitUcount(crate::userns::UcountKind),
    /// `fs.protected_symlinks`, `protected_hardlinks`, `protected_fifos`, `protected_regular`.
    Protected(wasixv2_vfs::Protection),
    /// `/proc/pressure/{cpu,io,memory}`: nothing stalls the system.
    Pressure,
    /// `/proc/modules`, `kallsyms`, `timer_list`, `buddyinfo`, `zoneinfo` and `crypto`.
    Modules,
    Kallsyms,
    TimerList,
    Buddyinfo,
    Zoneinfo,
    Crypto,
    /// `/proc/cgroups`.
    Cgroups,
    /// `/proc/kmsg`, which opens as the kernel log.
    Kmsg,
    /// `kernel.printk`, `kernel.dmesg_restrict` and `kernel.printk_devkmsg`.
    Printk,
    DmesgRestrict,
    PrintkDevkmsg,
    Acct,
    SeccompActionsAvail,
    SeccompActionsLogged,
    /// `kernel.core_pattern`, `kernel.core_uses_pid` and `kernel.core_pipe_limit`.
    CorePattern,
    CoreUsesPid,
    CorePipeLimit,
    /// One of the integer sysctls of [`crate::sysctl::Tunable`].
    Tunable(crate::sysctl::Tunable),
    /// `kernel.random.boot_id`, `uuid`, `entropy_avail` and `poolsize`.
    BootId,
    Uuid,
    EntropyAvail,
    Poolsize,
    /// `fs.file-nr`.
    FileNr,
    /// `vm.drop_caches`.
    DropCaches,
    /// `/proc/sysrq-trigger`.
    SysrqTrigger,
    /// `/proc/swaps`, `partitions`, `diskstats`, `devices`, `consoles`, `vmstat`,
    /// `interrupts` and `softirqs`.
    Swaps,
    Partitions,
    Diskstats,
    Devices,
    Consoles,
    Vmstat,
    Interrupts,
    Softirqs,
    KeySysctl(crate::keys::KeySysctl),
}

/// The files of a network namespace (`/proc/<pid>/net`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Net {
    /// `tcp`, or `tcp6`.
    Tcp(bool),
    Udp(bool),
    /// `icmp`/`icmp6` (ping sockets) and `raw`/`raw6`.
    Icmp(bool),
    Raw(bool),
    Unix,
    Dev,
    /// `route`, `ipv6_route` and `if_inet6`.
    Route,
    Ipv6Route,
    IfInet6,
    /// `igmp` and `igmp6`.
    Igmp,
    Igmp6,
    /// `mcfilter` and `mcfilter6`.
    Mcfilter(bool),
    /// `sctp/assocs`, `sctp/eps` and `sctp/snmp`.
    SctpAssocs,
    SctpEps,
    SctpSnmp,
}

impl Global {
    /// Tunables are writable by root; the rest are read-only.
    pub(super) fn mode(self) -> u32 {
        match self {
            Self::Hostname
            | Self::Domainname
            | Self::Overcommit
            | Self::OvercommitRatio
            | Self::OvercommitKbytes
            | Self::MaxMapCount
            | Self::MmapMinAddr
            | Self::FileMax
            | Self::NrOpen
            | Self::PipeMaxSize
            | Self::DirNotifyEnable
            | Self::LeasesEnable
            | Self::LeaseBreakTime
            | Self::KeySysctl(_)
            | Self::AioMaxNr
            | Self::Printk
            | Self::DmesgRestrict
            | Self::PrintkDevkmsg
            | Self::SeccompActionsLogged
            | Self::Acct
            | Self::CorePattern
            | Self::CoreUsesPid
            | Self::CorePipeLimit

            | Self::InitUcount(_)
            | Self::Protected(_)
            | Self::Tunable(_)
            | Self::Tainted
            | Self::PtraceScope
            // Who may write these is the namespace's business, not the mode's
            // (`pid_table_root_permissions`, `set_permissions`): see `ProcNode::permission`.
            | Self::PidMax
            | Self::Ucount(_) => 0o644,
            // `pid_ns_ctl_handler` checks the writer itself.
            Self::NsLastPid => 0o666,
            Self::DropCaches | Self::SysrqTrigger => 0o200,
            Self::Kmsg => 0o400,
            Self::Pressure => 0o666,
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
    /// The maps of the process's user namespace (`proc_uid_map_operations` and its twins).
    UidMap,
    GidMap,
    ProjidMap,
    Setgroups,
    /// The offsets of the process's time namespace for children.
    TimensOffsets,
    OomScore,
    OomScoreAdj,
    OomAdj,
    Personality,
    Wchan,
    Stack,
    TimerslackNs,
    Schedstat,
    Sched,
    Loginuid,
    Sessionid,
    Mountstats,
    Smaps,
    SmapsRollup,
    Io,
    Syscall,
    Auxv,
    Timers,
    /// `cgroup`.
    Cgroup,
    /// `coredump_filter`.
    CoredumpFilter,
}

/// A system file as a reader in the namespaces `ns`, the pid namespace `pid_ns` and the user
/// namespace `user_ns` sees it.
pub(super) fn global(
    source: &Source,
    ns: &NsProxy,
    pid_ns: &PidNs,
    user_ns: &UserNs,
    file: Global,
) -> FsResult<Vec<u8>> {
    let line = |s: &str| format!("{s}\n").into_bytes();
    let names = *ns.uts.lock_or_recover();
    let uts = |field: &[u8; 65]| {
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..len]).into_owned()
    };
    Ok(match file {
        Global::Cpuinfo => cpuinfo(),
        Global::Meminfo => meminfo(source),
        // `uptime_proc_show`: the boot-time clock of the reader's time namespace.
        Global::Uptime => {
            let up = timens_add(source.host.monotonic(), ns.time.offsets().1);
            line(&format!("{} 0.00", centis(up)))
        }
        // The last pid is the reader's pid namespace's.
        Global::Loadavg => {
            let (tasks, _) = source.task_counts();
            let last = pid_ns.last_pid();
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
        Global::PidMax => line(&pid_ns.pid_max().to_string()),
        Global::NsLastPid => line(&pid_ns.last_pid().to_string()),
        Global::Overcommit => line(&source.vm.overcommit_memory().to_string()),
        Global::OvercommitRatio => line(&source.vm.overcommit_ratio().to_string()),
        Global::OvercommitKbytes => line(&source.vm.overcommit_kbytes().to_string()),
        Global::MaxMapCount => line(&source.vm.max_map_count().to_string()),
        Global::MmapMinAddr => line(&crate::mm::MMAP_MIN_ADDR.to_string()),
        Global::FileMax => line(&i64::MAX.to_string()),
        Global::NrOpen => line(&source.nr_open.get().to_string()),
        Global::PipeMaxSize => line(&source.pipe_max_size.get().to_string()),
        Global::DirNotifyEnable => line(&source.sysctls.dir_notify_enable().to_string()),
        Global::LeasesEnable => line(&source.sysctls.leases_enable().to_string()),
        Global::LeaseBreakTime => line(&source.sysctls.lease_break_time().to_string()),
        Global::AioNr => line(&source.aio.nr().to_string()),
        Global::AioMaxNr => line(&source.aio.max().to_string()),
        Global::KeySysctl(entry) => line(&source.keys.sysctl(entry).to_string()),
        Global::OverflowUid | Global::OverflowGid => line(&OVERFLOW_ID.to_string()),
        Global::CapLastCap => line(&wasixv2_vfs::Cap::LAST.to_string()),
        Global::Tainted => line(&source.taint.get().to_string()),
        Global::PtraceScope => line(&source.yama.scope().to_string()),
        // The node renders them for its opener.
        Global::Keys | Global::KeyUsers => unreachable!("rendered for the opener"),
        Global::Ucount(kind) => line(&user_ns.ucount_max(kind).to_string()),
        Global::InitUcount(kind) => line(&initial(user_ns).ucount_max(kind).to_string()),
        Global::Protected(which) => {
            let vfs = source.vfs.upgrade().ok_or(Errno::ENODEV)?;
            line(&vfs.protected(which).to_string())
        }
        Global::Pressure => crate::cgroup::cpu::no_pressure().into_bytes(),
        // No modules, and no kernel image whose symbols could be listed.
        Global::Modules | Global::Kallsyms => Vec::new(),
        Global::TimerList => timer_list(source),
        Global::Buddyinfo => buddyinfo(source),
        Global::Zoneinfo => zoneinfo(source),
        Global::Crypto => crypto(),
        Global::Cgroups => cgroups(source),
        Global::Kmsg => Vec::new(),
        Global::Acct => {
            let values = source.acct.parameters.lock_or_recover();
            line(&format!("{}\t{}\t{}", values[0], values[1], values[2]))
        }
        Global::SeccompActionsAvail => line(&crate::seccomp::Logging::available()),
        Global::SeccompActionsLogged => line(&source.seccomp_logging.names()),
        Global::Printk => {
            let [console, message, minimum, default] = source.printk.levels();
            line(&format!("{console}\t{message}\t{minimum}\t{default}"))
        }
        Global::DmesgRestrict => line(&u8::from(source.printk.dmesg_restrict()).to_string()),
        Global::CorePattern => [source.coredump.pattern(), b"\n".to_vec()].concat(),
        Global::Tunable(tunable) => line(&source.tunables.get(tunable).to_string()),
        Global::BootId => line(&uuid_text(&source.boot_id)),
        Global::Uuid => {
            let mut id = [0; 16];
            source.host.random(&mut id);
            line(&uuid_text(&super::uuid_v4(id)))
        }
        // The host's entropy never runs out: the pool is always full.
        Global::EntropyAvail | Global::Poolsize => line("256"),
        // `proc_nr_files`: open files, none free, the maximum.
        Global::FileNr => line(&format!("{}\t0\t{}", source.nr_files(), i64::MAX)),
        Global::DropCaches | Global::SysrqTrigger => Vec::new(),
        // No swap, no block devices.
        Global::Swaps => b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n".to_vec(),
        Global::Partitions => b"major minor  #blocks  name\n\n".to_vec(),
        Global::Diskstats => Vec::new(),
        Global::Devices => devices(),
        // No console prints the kernel log.
        Global::Consoles => Vec::new(),
        Global::Vmstat => vmstat(source),
        Global::Interrupts => interrupts(),
        Global::Softirqs => softirqs(),
        Global::CoreUsesPid => line(&u8::from(source.coredump.uses_pid()).to_string()),
        Global::CorePipeLimit => line(&source.coredump.pipe_limit().to_string()),
        Global::PrintkDevkmsg => line(match source.printk.devkmsg() {
            crate::printk::Devkmsg::On => "on",
            crate::printk::Devkmsg::Off => "off",
            crate::printk::Devkmsg::Ratelimit => "ratelimit",
        }),
    })
}

/// A file of the network namespace `ns`.
pub(super) fn net(ns: &NetNs, file: Net) -> Vec<u8> {
    match file {
        Net::Tcp(v6) => sockets(ns, SocketKind::Stream, v6),
        Net::Udp(v6) => sockets(ns, SocketKind::Datagram, v6),
        Net::Icmp(v6) => icmp_sockets(ns, false, v6),
        Net::Raw(v6) => icmp_sockets(ns, true, v6),
        Net::Unix => unix_sockets(ns),
        Net::Dev => net_dev(ns),
        Net::Route => net_route(ns),
        Net::Ipv6Route => net_ipv6_route(ns),
        Net::IfInet6 => net_if_inet6(ns),
        Net::Igmp => net_igmp(ns),
        Net::Igmp6 => net_igmp6(ns),
        Net::Mcfilter(v6) => net_mcfilter(ns, v6),
        Net::SctpAssocs => sctp_assocs(ns),
        Net::SctpEps => sctp_eps(ns),
        Net::SctpSnmp => sctp_snmp(ns),
    }
}

/// `sctp_v4_seq_dump_addr` and `sctp_v6_seq_dump_addr` (`%pI4 `, `%pI6 `).
fn sctp_addr(out: &mut String, ip: IpAddr) {
    let _ = match ip {
        IpAddr::V4(v4) => write!(out, "{v4} "),
        IpAddr::V6(v6) => {
            let groups: Vec<String> = v6.segments().iter().map(|g| format!("{g:04x}")).collect();
            write!(out, "{} ", groups.join(":"))
        }
    };
}

/// `sctp_ep_hashfn`: the port into 64 buckets, `net_hash_mix` being 0.
fn sctp_bucket(port: u16) -> u16 {
    port & 63
}

/// `/proc/net/sctp/eps` (`sctp_eps_seq_show`): the listening endpoints by hash bucket, the
/// newest of a bucket first.
fn sctp_eps(ns: &NetNs) -> Vec<u8> {
    let mut eps = ns
        .network
        .as_ref()
        .map(|n| n.sctp_endpoints())
        .unwrap_or_default();
    eps.reverse();
    eps.sort_by_key(|e| sctp_bucket(e.port));
    let mut out = String::from(" ENDPT     SOCK   STY SST HBKT LPORT   UID INODE LADDRS\n");
    for e in &eps {
        let _ = write!(
            out,
            "{:8} {:8} {:<3} {:<3} {:<4} {:<5} {:5} {:5} ",
            0,
            0,
            e.style,
            e.state,
            sctp_bucket(e.port),
            e.port,
            e.uid,
            e.inode
        );
        for ip in &e.addrs {
            sctp_addr(&mut out, *ip);
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// `/proc/net/sctp/assocs` (`sctp_assocs_seq_show`).
fn sctp_assocs(ns: &NetNs) -> Vec<u8> {
    let assocs = ns
        .network
        .as_ref()
        .map(|n| n.sctp_assocs())
        .unwrap_or_default();
    let mut out = String::from(
        " ASSOC     SOCK   STY SST ST HBKT ASSOC-ID TX_QUEUE RX_QUEUE UID INODE LPORT RPORT LADDRS <-> RADDRS HBINT INS OUTS MAXRT T1X T2X RTXC wmema wmemq sndbuf rcvbuf\n",
    );
    for a in &assocs {
        let _ = write!(
            out,
            "{:8} {:8} {:<3} {:<3} {:<2} {:<4} {:4} {:8} {:8} {:7} {:5} {:<5} {:5} ",
            0,
            0,
            a.style,
            a.sk_state,
            a.state,
            0,
            a.id,
            a.tx_queue,
            a.rx_queue,
            a.uid,
            a.inode,
            a.lport,
            a.rport
        );
        out.push(' ');
        for ip in &a.laddrs {
            if Some(*ip) == a.primary_src {
                out.push('*');
            }
            sctp_addr(&mut out, *ip);
        }
        out.push_str("<-> ");
        for ip in &a.paddrs {
            if Some(*ip) == a.primary {
                out.push('*');
            }
            sctp_addr(&mut out, *ip);
        }
        // `sk_wmem_alloc` holds one reference of the socket's own.
        let _ = writeln!(
            out,
            "\t{:8} {:5} {:5} {:4} {:4} {:4} {:8} {:8} {:8} {:8} {:8}",
            a.hbinterval,
            a.incnt,
            a.outcnt,
            a.max_retrans,
            0,
            0,
            0,
            1,
            a.tx_queue,
            a.sndbuf,
            a.rcvbuf
        );
    }
    out.into_bytes()
}

/// `/proc/net/sctp/snmp` (`sctp_snmp_seq_show`).
fn sctp_snmp(ns: &NetNs) -> Vec<u8> {
    let counters = ns
        .network
        .as_ref()
        .map(|n| n.sctp_snmp())
        .unwrap_or_default();
    let mut out = String::new();
    for (name, value) in counters {
        let _ = writeln!(out, "{name:<32}\t{value}");
    }
    out.into_bytes()
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

/// The initial user namespace, at the top of `ns`'s line.
fn initial(ns: &UserNs) -> &UserNs {
    let mut at = ns;
    while let Some(parent) = at.parent() {
        at = parent;
    }
    at
}

/// The bytes the processes' address spaces hold.
fn memory_used(source: &Source) -> u64 {
    source
        .all_pids()
        .into_iter()
        .filter_map(|pid| source.mm(Scope { pid, tid: None }))
        .map(|mm| mm.size())
        .sum()
}

/// The free pages, by the order of the largest blocks they make (`MAX_PAGE_ORDER` 10): what
/// is free is one run, whole blocks of 4 MiB and then a block of each order its rest holds.
fn free_blocks(source: &Source) -> [u64; 11] {
    let free = ADDRESS_SPACE.saturating_sub(memory_used(source)) / crate::mm::PAGE_SIZE;
    let mut orders = [0; 11];
    orders[10] = free >> 10;
    for (order, count) in orders.iter_mut().enumerate().take(10) {
        *count = (free >> order) & 1;
    }
    orders
}

/// `frag_show`: one zone, `Normal`, of node 0.
fn buddyinfo(source: &Source) -> Vec<u8> {
    let mut out = String::from("Node 0, zone   Normal ");
    for count in free_blocks(source) {
        let _ = write!(out, "{count:>7}");
    }
    out.push('\n');
    out.into_bytes()
}

/// `zoneinfo_show` for node 0's one zone, with the watermarks `min_free_kbytes` gives.
fn zoneinfo(source: &Source) -> Vec<u8> {
    let page = crate::mm::PAGE_SIZE;
    let spanned = ADDRESS_SPACE / page;
    let free = ADDRESS_SPACE.saturating_sub(memory_used(source)) / page;
    let min_kb = source
        .tunables
        .get(crate::sysctl::Tunable::MinFreeKbytes)
        .max(0) as u64;
    let min = min_kb * 1024 / page;
    let (low, high) = (min + min / 4, min + min / 2);
    let mut out = String::from("Node 0, zone   Normal\n");
    let _ = writeln!(out, "  pages free     {free}");
    for (name, value) in [
        ("boost", 0),
        ("min", min),
        ("low", low),
        ("high", high),
        ("promo", high + min / 4),
        ("spanned", spanned),
        ("present", spanned),
        ("managed", spanned),
        ("cma", 0),
    ] {
        let _ = writeln!(out, "        {name:<9}{value}");
    }
    let _ = writeln!(out, "        protection: (0, 0, 0, 0)");
    for (name, value) in [
        ("nr_free_pages", free),
        ("nr_zone_inactive_anon", 0),
        ("nr_zone_active_anon", spanned - free),
        ("nr_zone_inactive_file", 0),
        ("nr_zone_active_file", 0),
        ("nr_zone_unevictable", 0),
        ("nr_zone_write_pending", 0),
        ("nr_mlock", 0),
        ("nr_bounce", 0),
        ("nr_free_cma", 0),
    ] {
        let _ = writeln!(out, "      {name} {value}");
    }
    out += "  pagesets\n";
    for cpu in 0..cpus() {
        let _ = write!(
            out,
            "    cpu: {cpu}\n              count:    0\n              high:     0\n\
             \x20             batch:    63\n  vm stats threshold: 0\n"
        );
    }
    out += "  node_unreclaimable:  0\n  start_pfn:           0\n";
    out.into_bytes()
}

/// `timer_list_show`: the clock bases of each CPU, none with an active timer the reader could
/// see (the kernel's timers are host sleeps), and the tick devices.
fn timer_list(source: &Source) -> Vec<u8> {
    const BASES: [(&str, &str); 8] = [
        ("ktime_get", "monotonic"),
        ("ktime_get_real", "realtime"),
        ("ktime_get_boottime", "boottime"),
        ("ktime_get_clocktai", "tai"),
        ("ktime_get", "monotonic"),
        ("ktime_get_real", "realtime"),
        ("ktime_get_boottime", "boottime"),
        ("ktime_get_clocktai", "tai"),
    ];
    let now = source.host.monotonic().as_nanos();
    let mut out =
        format!("Timer List Version: v0.10\nHRTIMER_MAX_CLOCK_BASES: 8\nnow at {now} nsecs\n\n");
    for cpu in 0..cpus() {
        let _ = writeln!(out, "cpu: {cpu}");
        for (index, (get_time, _)) in BASES.iter().enumerate() {
            let _ = write!(
                out,
                " clock {index}:\n  .base:       0\n  .index:      {index}\n  .resolution: 1 nsecs\n  \
                 .get_time:   {get_time}\n  .offset:     0 nsecs\nactive timers:\n"
            );
        }
        out += "  .expires_next   : 9223372036854775807 nsecs\n  .hres_active    : 1\n\n";
    }
    out += "Tick Device: mode:     1\nBroadcast device\nClock Event Device: <NULL>\n\n";
    for cpu in 0..cpus() {
        let _ = write!(
            out,
            "Tick Device: mode:     1\nPer CPU device: {cpu}\nClock Event Device: <NULL>\n\n"
        );
    }
    out.into_bytes()
}

/// `crypto_show`: the generic algorithms every kernel has built in.
fn crypto() -> Vec<u8> {
    let mut out = String::new();
    for (name, driver, kind, block, digest) in [
        ("crc32c", "crc32c-generic", "shash", 1, 4),
        ("sha256", "sha256-generic", "shash", 64, 32),
        ("sha1", "sha1-generic", "shash", 64, 20),
        ("md5", "md5-generic", "shash", 64, 16),
    ] {
        let _ = write!(
            out,
            "name         : {name}\ndriver       : {driver}\nmodule       : kernel\n\
             priority     : 100\nrefcnt       : 1\nselftest     : passed\ninternal     : no\n\
             type         : {kind}\nblocksize    : {block}\ndigestsize   : {digest}\n\n"
        );
    }
    out.into_bytes()
}

/// Memory is the 4 GiB a wasm32 guest can address, less what the processes use, as `sysinfo`
/// counts it.
pub(super) fn meminfo(source: &Source) -> Vec<u8> {
    let used: u64 = source
        .all_pids()
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
        ("CommitLimit", source.vm.commit_limit_kb(ADDRESS_SPACE)),
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
        // A listener's send queue is empty (`write_seq - snd_una`), its receive queue the
        // accept queue (`sk_ack_backlog`).
        let tx_queue = if port.listening { 0 } else { port.tx_queue };
        let _ = writeln!(
            out,
            "{sl:4}: {} {} {state:02X} {tx_queue:08X}:{:08X} 00:00000000 00000000 {:5}        0 {}",
            hex_addr(port.local),
            hex_addr(peer),
            port.rx_queue,
            port.uid,
            port.inode,
        );
    }
    out.into_bytes()
}

/// `ping_v4_seq_show`, `raw_seq_show` and their IPv6 forms (`ip6_dgram_sock_seq_show`): the
/// namespace's ping or raw ICMP sockets of one family by hash bucket, each chain newest
/// first. The buckets are those of a namespace whose `hash_mix` is 0: a ping socket's is its
/// identifier's low 6 bits (`ping_hashfn`), a raw socket's `hash_32` of its protocol
/// (`raw_hashfunc`). Every socket here is hashed once, so its `sk_refcnt` is 2.
fn icmp_sockets(ns: &NetNs, raw: bool, v6: bool) -> Vec<u8> {
    let domain = if v6 { Domain::Inet6 } else { Domain::Inet };
    let mut entries: Vec<_> = ns
        .network
        .as_ref()
        .map(|net| net.icmp_sockets())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.raw == raw && e.domain == domain)
        .collect();
    let bucket = |e: &IcmpEntry| match raw {
        true => u32::from(e.num).wrapping_mul(0x61C8_8647) >> 24,
        false => u32::from(e.num) & 63,
    };
    entries.sort_by_key(bucket);
    let header = match v6 {
        false => {
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops"
        }
        true => {
            "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops"
        }
    };
    // Only `/proc/net/icmp` pads its lines (`seq_setwidth(seq, 127)`, `seq_pad`).
    let pad = !raw && !v6;
    let mut out = String::new();
    let mut line = |text: String| match pad {
        true => {
            let _ = writeln!(out, "{text:<127}");
        }
        false => {
            let _ = writeln!(out, "{text}");
        }
    };
    line(header.to_string());
    for e in &entries {
        let any = match domain {
            Domain::Inet => IpAddr::from([0u8; 4]),
            Domain::Inet6 => IpAddr::from([0u16; 8]),
        };
        let local = SocketAddr::new(e.local.unwrap_or(any), e.num);
        // A raw socket's remote port is always 0.
        let (peer, dport) = e.peer.unwrap_or((any, 0));
        let remote = SocketAddr::new(peer, if raw { 0 } else { dport });
        // TCP_ESTABLISHED once connected, else TCP_CLOSE.
        let state = if e.peer.is_some() { 0x01 } else { 0x07 };
        let sl = match (raw, v6) {
            (true, false) => format!("{:4}", bucket(e)),
            _ => format!("{:5}", bucket(e)),
        };
        line(format!(
            "{sl}: {} {} {state:02X} 00000000:{:08X} 00:00000000 00000000 {:5} {:8} {} 2 0 {}",
            hex_addr(local),
            hex_addr(remote),
            e.rx_queue,
            e.uid,
            0,
            e.inode,
            e.drops,
        ));
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

/// `igmp_mc_seq_show`: the IPv4 groups each interface is in, newest first: those sockets
/// joined, and the all-hosts group an up interface is in (`ip_mc_up`). No reports are sent,
/// so no timer runs and the interface is no group's reporter.
fn net_igmp(ns: &NetNs) -> Vec<u8> {
    use std::net::IpAddr;
    let interfaces = ns
        .network
        .as_ref()
        .map(|n| n.interfaces())
        .unwrap_or_default();
    let mut out = String::from("Idx\tDevice    : Count Querier\tGroup    Users Timer\tReporter\n");
    for iface in &interfaces {
        let groups: Vec<_> = iface
            .groups
            .iter()
            .filter_map(|(g, users)| match g {
                IpAddr::V4(v4) => Some((*v4, *users)),
                IpAddr::V6(_) => None,
            })
            .collect();
        if groups.is_empty() {
            continue;
        }
        let _ = writeln!(
            out,
            "{}\t{:<10}: {:>5} {:>7}",
            iface.index,
            iface.name,
            groups.len(),
            "V3"
        );
        for (group, users) in groups {
            // The big-endian word, printed as the native one it is read as.
            let word = u32::from_le_bytes(group.octets());
            let _ = writeln!(out, "\t\t\t\t{word:08X} {users:>5} 0:00000000\t\t0");
        }
    }
    out.into_bytes()
}

/// `igmp6_mc_seq_show`: the IPv6 groups each interface is in, newest first: those sockets
/// joined, and the kernel's own while it is up with IPv6 (all-nodes, `ff02::1` and
/// interface-local `ff01::1`, and each address's solicited-node group, not on loopback). The
/// all-nodes and interface-local groups are never reported (`MAF_NOREPORT`); the interface
/// was the last to report the others (`MAF_LAST_REPORTER`).
fn net_igmp6(ns: &NetNs) -> Vec<u8> {
    use std::net::{IpAddr, Ipv6Addr};
    /// `MAF_NOREPORT | MAF_LAST_REPORTER` and `MAF_LAST_REPORTER`.
    const NOREPORT: u32 = 0x0c;
    const REPORTED: u32 = 0x04;
    let interfaces = ns
        .network
        .as_ref()
        .map(|n| n.interfaces())
        .unwrap_or_default();
    let all_nodes = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
    let mut out = String::new();
    for iface in &interfaces {
        for (group, users) in &iface.groups {
            let IpAddr::V6(group) = *group else {
                continue;
            };
            // Below link scope, or all-nodes (`igmp6_join_group`).
            let flags = match group == all_nodes || group.segments()[0] & 0xf < 2 {
                true => NOREPORT,
                false => REPORTED,
            };
            let _ = writeln!(
                out,
                "{:<4} {:<15} {:032x} {users:>5} {flags:08X} 0",
                iface.index,
                iface.name,
                u128::from(group),
            );
        }
    }
    out.into_bytes()
}

/// `igmp_mcf_seq_show`, `igmp6_mcf_seq_show`: every source of every group of every interface,
/// with how many memberships include and exclude it (the name cut to 6 characters). As
/// `igmp_mcf_get_first` and `igmp6_mcf_get_first` find the first row, the listing starts at
/// the first interface whose newest group has sources; from there every group's are listed.
fn net_mcfilter(ns: &NetNs, v6: bool) -> Vec<u8> {
    use std::net::IpAddr;
    let interfaces = ns
        .network
        .as_ref()
        .map(|n| n.interfaces())
        .unwrap_or_default();
    let mut out = String::from(match v6 {
        false => "Idx Device        MCA        SRC    INC    EXC\n",
        true => {
            "Idx Device                Multicast Address                   Source Address    INC    EXC\n"
        }
    });
    let family = |g: &IpAddr| g.is_ipv6() == v6;
    let start = interfaces.iter().position(|iface| {
        let newest = iface.sources.iter().find(|g| family(&g.group));
        newest.is_some_and(|g| !g.sources.is_empty())
    });
    for iface in interfaces.iter().skip(start.unwrap_or(interfaces.len())) {
        let name: String = iface.name.chars().take(6).collect();
        for g in iface.sources.iter().filter(|g| family(&g.group)) {
            for &(src, inc, exc) in &g.sources {
                // `ntohl` of IPv4's words, `%pi6` for IPv6's.
                let addrs = match (g.group, src) {
                    (IpAddr::V4(group), IpAddr::V4(src)) => {
                        format!("0x{:08x} 0x{:08x}", u32::from(group), u32::from(src))
                    }
                    (IpAddr::V6(group), IpAddr::V6(src)) => {
                        format!("{:032x} {:032x}", u128::from(group), u128::from(src))
                    }
                    _ => continue,
                };
                let _ = writeln!(out, "{:3} {name:>6} {addrs} {inc:6} {exc:6}", iface.index);
            }
        }
    }
    out.into_bytes()
}

/// `%pU`: a UUID in its lowercase text form.
fn uuid_text(id: &[u8; 16]) -> String {
    let hex: Vec<String> = id.iter().map(|b| format!("{b:02x}")).collect();
    let hex = hex.concat();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// `chrdev_show` and `blkdev_show`: the drivers' majors.
fn devices() -> Vec<u8> {
    let mut out = String::from("Character devices:\n");
    for (major, name) in [
        (1, "mem"),
        (5, "/dev/tty"),
        (5, "/dev/ptmx"),
        (136, "pts"),
        (229, "hvc"),
    ] {
        let _ = writeln!(out, "{major:3} {name}");
    }
    out.push_str("\nBlock devices:\n");
    out.into_bytes()
}

/// `vmstat_show`: the memory of `meminfo` in pages; there is no page cache, swap or reclaim,
/// and the guest's memory never faults.
fn vmstat(source: &Source) -> Vec<u8> {
    let used: u64 = source
        .all_pids()
        .into_iter()
        .filter_map(|pid| source.mm(Scope { pid, tid: None }))
        .map(|mm| mm.size())
        .sum();
    let page = 4096;
    let free = ADDRESS_SPACE.saturating_sub(used) / page;
    let anon = used / page;
    let mut out = String::new();
    for (name, value) in [
        ("nr_free_pages", free),
        ("nr_zone_inactive_anon", 0),
        ("nr_zone_active_anon", anon),
        ("nr_zone_inactive_file", 0),
        ("nr_zone_active_file", 0),
        ("nr_zone_unevictable", 0),
        ("nr_mlock", 0),
        ("nr_inactive_anon", 0),
        ("nr_active_anon", anon),
        ("nr_inactive_file", 0),
        ("nr_active_file", 0),
        ("nr_unevictable", 0),
        ("nr_anon_pages", anon),
        ("nr_mapped", 0),
        ("nr_file_pages", 0),
        ("nr_dirty", 0),
        ("nr_writeback", 0),
        ("nr_shmem", 0),
        ("nr_dirty_threshold", 0),
        ("nr_dirty_background_threshold", 0),
        ("pgpgin", 0),
        ("pgpgout", 0),
        ("pswpin", 0),
        ("pswpout", 0),
        ("pgfault", 0),
        ("pgmajfault", 0),
        ("oom_kill", 0),
    ] {
        let _ = writeln!(out, "{name} {value}");
    }
    out.into_bytes()
}

/// The `CPU0 CPU1 ...` header of `interrupts` and `softirqs`, after `indent` blanks.
fn cpu_header(indent: usize) -> String {
    let mut out = " ".repeat(indent);
    for cpu in 0..cpus() {
        let _ = write!(out, "CPU{cpu:<8}");
    }
    out.trim_end().to_string() + "\n"
}

/// `show_interrupts`: the CPUs, and no interrupt lines; the host takes them all.
fn interrupts() -> Vec<u8> {
    cpu_header(11).into_bytes()
}

/// `show_softirqs`: every kind, none raised.
fn softirqs() -> Vec<u8> {
    let mut out = cpu_header(20);
    for name in [
        "HI", "TIMER", "NET_TX", "NET_RX", "BLOCK", "IRQ_POLL", "TASKLET", "SCHED", "HRTIMER",
        "RCU",
    ] {
        let _ = write!(out, "{name:>12}:");
        for _ in 0..cpus() {
            out.push_str(&format!(" {:>10}", 0));
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// `proc_cgroupstats_show`: every controller, none of them on a v1 hierarchy (hierarchy 0),
/// with the default hierarchy's count of cgroups.
fn cgroups(source: &Source) -> Vec<u8> {
    let count = source.cgroups.lock().nr_cgroups();
    let mut out = String::from("#subsys_name\thierarchy\tnum_cgroups\tenabled\n");
    for name in crate::cgroup::legacy_names() {
        let _ = writeln!(out, "{name}\t0\t{count}\t1");
    }
    out.into_bytes()
}

/// A file of a process's directory, as a reader in the user namespace `reader` and the
/// namespaces `viewer` sees it.
pub(super) fn per_pid(
    source: &Source,
    scope: Scope,
    file: PerPid,
    reader: &Arc<UserNs>,
    viewer: &NsProxy,
) -> FsResult<Vec<u8>> {
    let info = source.process(scope.pid).ok_or(Errno::ESRCH)?;
    let process = &info.process;
    let cred = source
        .cred_of(scope.tid.unwrap_or(scope.pid))
        .ok_or(Errno::ESRCH)?;
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
            // `mounts_open_common`: `EINVAL` for a task that is done.
            let ns = task.ns.as_ref().ok_or(Errno::EINVAL)?;
            mounts(source, &ns.mnt.tree, &root, file == PerPid::Mountinfo)
        }
        PerPid::Stat => stat_line(source, scope, &info, &viewer.time),
        PerPid::Statm => {
            let pages = source.mm(scope).map_or(0, |mm| mm.size()) / PAGE_SIZE;
            format!("{pages} {pages} 0 0 0 {pages} 0\n").into_bytes()
        }
        PerPid::Status => status(source, scope, &info, reader, &cred),
        PerPid::UidMap => cred.user_ns.show_map(MapKind::Uid, reader),
        PerPid::GidMap => cred.user_ns.show_map(MapKind::Gid, reader),
        PerPid::ProjidMap => cred.user_ns.show_map(MapKind::Projid, reader),
        PerPid::Setgroups => cred.user_ns.show_setgroups(),
        PerPid::TimensOffsets => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            match &task.ns {
                Some(ns) => timens_offsets(&ns.time_for_children),
                None => Vec::new(),
            }
        }
        PerPid::OomScore => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            introspect::oom_score(process, &task, source.mm(scope).as_deref())
        }
        PerPid::OomScoreAdj => {
            format!("{}\n", process.ident.lock_or_recover().oom_score_adj).into_bytes()
        }
        PerPid::OomAdj => {
            let adj = process.ident.lock_or_recover().oom_score_adj;
            format!("{}\n", introspect::oom_adj(adj)).into_bytes()
        }
        PerPid::Personality => {
            format!("{:08x}\n", process.ident.lock_or_recover().personality).into_bytes()
        }
        // `proc_pid_wchan`: no kernel symbol names a host wait.
        PerPid::Wchan => b"0".to_vec(),
        // `proc_pid_stack`: there is no kernel stack to show.
        PerPid::Stack => Vec::new(),
        PerPid::TimerslackNs => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            format!("{}\n", task.sched.timer_slack()).into_bytes()
        }
        PerPid::Schedstat => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            introspect::schedstat(&task)
        }
        PerPid::Sched => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            let pid = source.nr(scope.tid.unwrap_or(scope.pid));
            let threads = source.threads(scope.pid).len();
            introspect::sched(task.comm.name(), pid, threads, &task)
        }
        PerPid::Loginuid => introspect::loginuid(process, reader),
        PerPid::Sessionid => introspect::sessionid(process),
        PerPid::Mountstats => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            let root = task.fs.lock_or_recover().root.clone();
            let ns = task.ns.as_ref().ok_or(Errno::EINVAL)?;
            introspect::mountstats(source, &ns.mnt.tree, &root)
        }
        PerPid::Smaps => introspect::smaps(source.mm(scope).as_deref(), false),
        PerPid::SmapsRollup => introspect::smaps(source.mm(scope).as_deref(), true),
        PerPid::Io => introspect::io(source, scope, process)?,
        PerPid::Syscall => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            introspect::syscall(&task)
        }
        PerPid::Auxv => process.auxv.lock_or_recover().clone(),
        PerPid::Timers => {
            let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
            introspect::timers(&task, source.nr(scope.pid), |id| source.nr(id))
        }
        // `proc_coredump_filter_read`.
        PerPid::CoredumpFilter => {
            let filter = process.ident.lock_or_recover().coredump_filter;
            format!("{filter:08x}\n").into_bytes()
        }
        // `proc_cgroup_show`: the path as seen from the reader's cgroup namespace; a zombie's
        // is the one it exited in.
        PerPid::Cgroup => {
            let task = source.task(scope.pid, scope.tid).map(|task| task.cgroup);
            let cgroup = task.or(info.cgroup.clone()).ok_or(Errno::ESRCH)?;
            let root = &viewer.cgroup.root;
            source.cgroups.proc_line(&cgroup, root).unwrap_or_default()
        }
    })
}

/// `proc_timens_show_offsets`: `show_offset` of each clock, as a normalised timespec.
fn timens_offsets(ns: &TimeNs) -> Vec<u8> {
    let (monotonic, boottime) = ns.offsets();
    let mut out = String::new();
    for (name, offset) in [("monotonic", monotonic), ("boottime", boottime)] {
        let (sec, nsec) = (
            offset.div_euclid(1_000_000_000),
            offset.rem_euclid(1_000_000_000),
        );
        let _ = writeln!(out, "{name:<10} {sec:>10} {nsec:>9}");
    }
    out.into_bytes()
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
        let flag = |on: bool, c: char| if on { c } else { '-' };
        let perms = format!(
            "{}{}-{}",
            flag(region.read, 'r'),
            flag(region.write, 'w'),
            if region.shared { 's' } else { 'p' }
        );
        let line = format!(
            "{:08x}-{:08x} {perms} 00000000 00:00 0",
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
            out.extend_from_slice(b" - ");
            out.extend_from_slice(&escape(&sb.fs.type_name()));
            out.push(b' ');
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
            out.push(b' ');
            out.extend_from_slice(&escape(&sb.fs.type_name()));
            let _ = write!(out_string(&mut out), " {options}");
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
pub(super) fn escape(path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len());
    for &b in path {
        match b {
            b' ' | b'\t' | b'\n' | b'\\' => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            _ => out.push(b),
        }
    }
    out
}

/// The task's state letter: a zombie, in a ptrace stop, stopped by a stop signal, or sleeping
/// (a task reading its own state is running, but a file does not know who reads it).
fn state(info: &super::ProcessInfo, task: Option<&super::TaskInfo>) -> (char, &'static str) {
    match info.zombie {
        true => ('Z', "zombie"),
        false if task.is_some_and(|task| task.signals.is_traced()) => ('t', "tracing stop"),
        false if task.is_some_and(|task| task.signals.is_stopped()) => ('T', "stopped"),
        false => ('S', "sleeping"),
    }
}

/// `do_task_stat`. The start time is on the boot-time clock of the reader's time namespace
/// `reader_time`.
fn stat_line(
    source: &Source,
    scope: Scope,
    info: &super::ProcessInfo,
    reader_time: &TimeNs,
) -> Vec<u8> {
    let process = &info.process;
    let pid = source.nr(scope.tid.unwrap_or(scope.pid));
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
    let start = ticks(timens_add(process.start, reader_time.offsets().1));
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
            let tpgid = tty.pgrp().map_or(0, |pgrp| pgrp.nr(&source.pid_ns));
            (i128::from(tty_nr), i128::from(tpgid))
        }
        None => (0, -1),
    };
    let fields: Vec<String> = [
        source.nr(info.parent) as i128,    // ppid
        source.nr(ident.pgid) as i128,     // pgrp
        source.nr(ident.sid) as i128,      // session
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
/// `proc_pid_status`: the ids as the reader's user namespace `reader` sees them.
fn status(
    source: &Source,
    scope: Scope,
    info: &super::ProcessInfo,
    reader: &UserNs,
    cred: &crate::cred::Cred,
) -> Vec<u8> {
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
    let pid = source.nr(scope.tid.unwrap_or(scope.pid));
    // Each pid from the instance's pid namespace down to the task's own.
    let upids = source.upids(scope.tid.unwrap_or(scope.pid));
    let in_each = |id: u32| -> String {
        let nrs = upids.iter().map(|upid| source.pids.nr(&upid.ns, id));
        nrs.map(|nr| nr.to_string()).collect::<Vec<_>>().join("\t")
    };
    let ns_pid: Vec<String> = upids.iter().map(|upid| upid.nr.to_string()).collect();
    let kb = task.as_ref().map_or(0, |task| task.mm.size()) / 1024;
    let locked_kb = task.as_ref().map_or(0, |task| task.mm.locked()) / 1024;
    let threads = source.threads(scope.pid).len().max(1);
    let (letter, state_name) = state(info, task.as_ref());
    let user = cred.user;
    let user = [user.real, user.effective, user.saved, user.fs].map(|id| reader.munged_uid(id));
    let group = cred.group;
    let group =
        [group.real, group.effective, group.saved, group.fs].map(|id| reader.munged_gid(id));
    let comm = task
        .as_ref()
        .map_or(Vec::new(), |task| task.comm.name().to_vec());
    let affinity = task
        .as_ref()
        .map_or(Vec::new(), |task| task.sched.affinity().to_vec());
    let groups: String = cred
        .groups
        .iter()
        .map(|&gid| format!("{} ", reader.munged_gid(gid)))
        .collect();
    let seccomp = task.as_ref().map_or((false, 0, 0), |task| task.seccomp);
    // `ptrace_parent`: the tracing task, by its pid in the reader's namespace.
    let tracer = task
        .as_ref()
        .and_then(|task| task.signals.tracer())
        .map_or(0, |tracer| source.nr(tracer.0));
    let cpus = cpus();
    let mut out = String::new();
    let _ = write!(
        out,
        "Name:\t{}\nUmask:\t{umask:04o}\nState:\t{letter} ({state_name})\nTgid:\t{}\nNgid:\t0\nPid:\t{pid}\nPPid:\t{}\nTracerPid:\t{tracer}\n\
         Uid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\nFDSize:\t{fd_size}\nGroups:\t{groups}\n\
         NStgid:\t{}\nNSpid:\t{}\nNSpgid:\t{}\nNSsid:\t{}\n\
         VmPeak:\t{kb:8} kB\nVmSize:\t{kb:8} kB\nVmLck:\t{locked_kb:8} kB\nVmPin:\t       0 kB\nVmHWM:\t{kb:8} kB\nVmRSS:\t{kb:8} kB\n\
         RssAnon:\t{kb:8} kB\nRssFile:\t       0 kB\nRssShmem:\t       0 kB\nVmData:\t{kb:8} kB\nVmStk:\t       0 kB\nVmExe:\t       0 kB\nVmLib:\t       0 kB\nVmPTE:\t       0 kB\nVmSwap:\t       0 kB\n\
         Threads:\t{threads}\nSigQ:\t0/4096\nSigPnd:\t{:016x}\nShdPnd:\t{:016x}\nSigBlk:\t{:016x}\nSigIgn:\t{:016x}\nSigCgt:\t{:016x}\n\
         CapInh:\t{:016x}\nCapPrm:\t{:016x}\nCapEff:\t{:016x}\nCapBnd:\t{:016x}\nCapAmb:\t{:016x}\n\
         NoNewPrivs:\t{}\nSeccomp:\t{}\nSeccomp_filters:\t{}\nCpus_allowed:\t{}\nCpus_allowed_list:\t{}\n\
         voluntary_ctxt_switches:\t0\nnonvoluntary_ctxt_switches:\t0\n",
        String::from_utf8_lossy(&comm),
        source.nr(scope.pid),
        source.nr(info.parent),
        user[0],
        user[1],
        user[2],
        user[3],
        group[0],
        group[1],
        group[2],
        group[3],
        in_each(scope.pid),
        ns_pid.join("\t"),
        in_each(ident.pgid),
        in_each(ident.sid),
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
        u8::from(seccomp.0),
        seccomp.1,
        seccomp.2,
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
    let mut info = format!(
        "pos:\t{}\nflags:\t0{flags:o}\nmnt_id:\t{}\nino:\t{}\n",
        file.pos(),
        path.mnt.id,
        path.dentry.node().ino()
    );
    if let Some(object) = file.object() {
        info += &object.show_fdinfo();
    }
    info.into_bytes()
}
