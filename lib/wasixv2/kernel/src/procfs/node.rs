//! The nodes of the process filesystem: what each name is, its inode number, metadata,
//! directory listing and link target.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::{Errno, Fd};
use wasixv2_vfs::{
    Attr, Cap, Ctx, DirEntry, DirSink, FileType, FsResult, Handle, Link, Node, OpenOptions,
    Protection, SetAttr,
};

use super::{Source, text};
use crate::sysctl::Tunable;
use crate::{
    aio::AioSysctl,
    cred::Cred,
    file::NrOpen,
    mm::VmSysctls,
    ns::{NsKind, NsLink, PidNs, TimensOffset, UserNs},
    pipe::PipeMaxSize,
    sync::MutexExt,
    taint::Taint,
    uidgid::{Kgid, Kuid},
    userns::MapKind,
    userns::UcountKind,
};

/// A directory, file or link of the process filesystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Root,
    /// One of [`STATIC`], by index.
    Static(usize),
    /// `/proc/<pid>`, or `/proc/<pid>/task/<tid>` with a thread.
    Dir(Scope),
    /// An entry of a process or thread directory, one of [`PID_ENTRIES`] by index.
    Pid(Scope, usize),
    /// `fd/<n>`.
    Fd(Scope, u32),
    /// `fdinfo/<n>`.
    FdInfo(Scope, u32),
    /// `ns/<name>`, one of [`NsLink::ALL`] by index.
    Ns(Scope, usize),
    /// `net/<name>`, one of [`NET_ENTRIES`] by index.
    Net(Scope, usize),
    /// `attr/<name>`: a generic LSM security attribute.
    Attr(Scope, usize),
}

/// Whose directory: a process (`/proc/<pid>`) or one of its threads
/// (`/proc/<pid>/task/<tid>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    pub(super) pid: u32,
    pub(super) tid: Option<u32>,
}

/// What a static node is.
#[derive(Clone, Copy)]
enum Static {
    Dir,
    File(text::Global),
    /// A symlink with a fixed body.
    Link(&'static [u8]),
    /// `self`: the caller's process.
    SelfLink,
    /// `thread-self`: the caller's thread.
    ThreadSelf,
    /// `sys/net`: the sysctls of the opener's network namespace.
    SysNet,
}

/// The fixed part of the tree: parent index, name, kind. Index 0's parent is the root.
const ROOT: usize = usize::MAX;
const STATIC: &[(usize, &str, Static)] = &[
    (ROOT, "self", Static::SelfLink),
    (ROOT, "thread-self", Static::ThreadSelf),
    (ROOT, "mounts", Static::Link(b"self/mounts")),
    (ROOT, "cpuinfo", Static::File(text::Global::Cpuinfo)),
    (ROOT, "meminfo", Static::File(text::Global::Meminfo)),
    (ROOT, "uptime", Static::File(text::Global::Uptime)),
    (ROOT, "loadavg", Static::File(text::Global::Loadavg)),
    (ROOT, "stat", Static::File(text::Global::Stat)),
    (ROOT, "version", Static::File(text::Global::Version)),
    (ROOT, "filesystems", Static::File(text::Global::Filesystems)),
    (ROOT, "cmdline", Static::File(text::Global::Cmdline)),
    (ROOT, "sys", Static::Dir),
    (ROOT, "net", Static::Link(b"self/net")),
    // 13
    (11, "kernel", Static::Dir),
    (11, "vm", Static::Dir),
    (11, "fs", Static::Dir),
    // 16
    (13, "hostname", Static::File(text::Global::Hostname)),
    (13, "domainname", Static::File(text::Global::Domainname)),
    (13, "ostype", Static::File(text::Global::Ostype)),
    (13, "osrelease", Static::File(text::Global::Osrelease)),
    (13, "version", Static::File(text::Global::Version)),
    (13, "pid_max", Static::File(text::Global::PidMax)),
    (13, "ns_last_pid", Static::File(text::Global::NsLastPid)),
    (
        14,
        "overcommit_memory",
        Static::File(text::Global::Overcommit),
    ),
    (14, "mmap_min_addr", Static::File(text::Global::MmapMinAddr)),
    (15, "file-max", Static::File(text::Global::FileMax)),
    (15, "nr_open", Static::File(text::Global::NrOpen)),
    (15, "pipe-max-size", Static::File(text::Global::PipeMaxSize)),
    (11, "net", Static::SysNet),
    // 29
    (11, "user", Static::Dir),
    (
        29,
        "max_user_namespaces",
        ucount(UcountKind::Ns(NsKind::User)),
    ),
    (
        29,
        "max_pid_namespaces",
        ucount(UcountKind::Ns(NsKind::Pid)),
    ),
    (
        29,
        "max_uts_namespaces",
        ucount(UcountKind::Ns(NsKind::Uts)),
    ),
    (
        29,
        "max_ipc_namespaces",
        ucount(UcountKind::Ns(NsKind::Ipc)),
    ),
    (
        29,
        "max_net_namespaces",
        ucount(UcountKind::Ns(NsKind::Net)),
    ),
    (
        29,
        "max_mnt_namespaces",
        ucount(UcountKind::Ns(NsKind::Mnt)),
    ),
    (
        29,
        "max_cgroup_namespaces",
        ucount(UcountKind::Ns(NsKind::Cgroup)),
    ),
    (
        29,
        "max_time_namespaces",
        ucount(UcountKind::Ns(NsKind::Time)),
    ),
    (13, "overflowuid", Static::File(text::Global::OverflowUid)),
    (13, "overflowgid", Static::File(text::Global::OverflowGid)),
    (13, "cap_last_cap", Static::File(text::Global::CapLastCap)),
    (
        15,
        "dir-notify-enable",
        Static::File(text::Global::DirNotifyEnable),
    ),
    (
        15,
        "leases-enable",
        Static::File(text::Global::LeasesEnable),
    ),
    (
        15,
        "lease-break-time",
        Static::File(text::Global::LeaseBreakTime),
    ),
    (13, "tainted", Static::File(text::Global::Tainted)),
    (
        14,
        "overcommit_ratio",
        Static::File(text::Global::OvercommitRatio),
    ),
    (
        14,
        "overcommit_kbytes",
        Static::File(text::Global::OvercommitKbytes),
    ),
    (14, "max_map_count", Static::File(text::Global::MaxMapCount)),
    (ROOT, "keys", Static::File(text::Global::Keys)),
    (ROOT, "key-users", Static::File(text::Global::KeyUsers)),
    (15, "aio-nr", Static::File(text::Global::AioNr)),
    (15, "aio-max-nr", Static::File(text::Global::AioMaxNr)),
    // 52
    (13, "yama", Static::Dir),
    (52, "ptrace_scope", Static::File(text::Global::PtraceScope)),
    (ROOT, "cgroups", Static::File(text::Global::Cgroups)),
    (ROOT, "kmsg", Static::File(text::Global::Kmsg)),
    (13, "printk", Static::File(text::Global::Printk)),
    (
        13,
        "dmesg_restrict",
        Static::File(text::Global::DmesgRestrict),
    ),
    (
        13,
        "printk_devkmsg",
        Static::File(text::Global::PrintkDevkmsg),
    ),
    (13, "core_pattern", Static::File(text::Global::CorePattern)),
    (13, "core_uses_pid", Static::File(text::Global::CoreUsesPid)),
    (
        13,
        "core_pipe_limit",
        Static::File(text::Global::CorePipeLimit),
    ),
    // 62
    (13, "random", Static::Dir),
    (15, "inotify", Static::Dir),
    (13, "threads-max", tunable(Tunable::ThreadsMax)),
    (13, "sysrq", tunable(Tunable::Sysrq)),
    (13, "panic", tunable(Tunable::Panic)),
    (13, "panic_on_oops", tunable(Tunable::PanicOnOops)),
    (
        13,
        "sched_child_runs_first",
        tunable(Tunable::SchedChildRunsFirst),
    ),
    (
        13,
        "sched_rr_timeslice_ms",
        tunable(Tunable::SchedRrTimesliceMs),
    ),
    (13, "sched_rt_period_us", tunable(Tunable::SchedRtPeriodUs)),
    (
        13,
        "sched_rt_runtime_us",
        tunable(Tunable::SchedRtRuntimeUs),
    ),
    (
        62,
        "write_wakeup_threshold",
        tunable(Tunable::RandomWriteWakeupThreshold),
    ),
    (
        62,
        "urandom_min_reseed_secs",
        tunable(Tunable::RandomUrandomMinReseedSecs),
    ),
    (
        63,
        "max_queued_events",
        tunable(Tunable::InotifyMaxQueuedEvents),
    ),
    (14, "swappiness", tunable(Tunable::Swappiness)),
    (14, "dirty_ratio", tunable(Tunable::DirtyRatio)),
    (
        14,
        "dirty_background_ratio",
        tunable(Tunable::DirtyBackgroundRatio),
    ),
    (
        14,
        "dirty_expire_centisecs",
        tunable(Tunable::DirtyExpireCentisecs),
    ),
    (
        14,
        "dirty_writeback_centisecs",
        tunable(Tunable::DirtyWritebackCentisecs),
    ),
    (14, "vfs_cache_pressure", tunable(Tunable::VfsCachePressure)),
    (14, "min_free_kbytes", tunable(Tunable::MinFreeKbytes)),
    (14, "panic_on_oom", tunable(Tunable::PanicOnOom)),
    (
        14,
        "oom_kill_allocating_task",
        tunable(Tunable::OomKillAllocatingTask),
    ),
    (14, "oom_dump_tasks", tunable(Tunable::OomDumpTasks)),
    (14, "page-cluster", tunable(Tunable::PageCluster)),
    (14, "stat_interval", tunable(Tunable::StatInterval)),
    (14, "laptop_mode", tunable(Tunable::LaptopMode)),
    (
        14,
        "admin_reserve_kbytes",
        tunable(Tunable::AdminReserveKbytes),
    ),
    (
        14,
        "user_reserve_kbytes",
        tunable(Tunable::UserReserveKbytes),
    ),
    (62, "boot_id", Static::File(text::Global::BootId)),
    (62, "uuid", Static::File(text::Global::Uuid)),
    (
        62,
        "entropy_avail",
        Static::File(text::Global::EntropyAvail),
    ),
    (62, "poolsize", Static::File(text::Global::Poolsize)),
    (15, "file-nr", Static::File(text::Global::FileNr)),
    (14, "drop_caches", Static::File(text::Global::DropCaches)),
    (
        ROOT,
        "sysrq-trigger",
        Static::File(text::Global::SysrqTrigger),
    ),
    (ROOT, "swaps", Static::File(text::Global::Swaps)),
    (ROOT, "partitions", Static::File(text::Global::Partitions)),
    (ROOT, "diskstats", Static::File(text::Global::Diskstats)),
    (ROOT, "devices", Static::File(text::Global::Devices)),
    (ROOT, "consoles", Static::File(text::Global::Consoles)),
    (ROOT, "vmstat", Static::File(text::Global::Vmstat)),
    (ROOT, "interrupts", Static::File(text::Global::Interrupts)),
    (ROOT, "softirqs", Static::File(text::Global::Softirqs)),
    // 105
    (ROOT, "pressure", Static::Dir),
    (105, "cpu", Static::File(text::Global::Pressure)),
    (105, "io", Static::File(text::Global::Pressure)),
    (105, "memory", Static::File(text::Global::Pressure)),
    (
        29,
        "max_inotify_instances",
        ucount(UcountKind::InotifyInstances),
    ),
    (
        29,
        "max_inotify_watches",
        ucount(UcountKind::InotifyWatches),
    ),
    (
        63,
        "max_user_instances",
        Static::File(text::Global::InitUcount(UcountKind::InotifyInstances)),
    ),
    (
        63,
        "max_user_watches",
        Static::File(text::Global::InitUcount(UcountKind::InotifyWatches)),
    ),
    (15, "protected_symlinks", protected(Protection::Symlinks)),
    (15, "protected_hardlinks", protected(Protection::Hardlinks)),
    (15, "protected_fifos", protected(Protection::Fifos)),
    (15, "protected_regular", protected(Protection::Regular)),
    (15, "suid_dumpable", tunable(Tunable::SuidDumpable)),
    (ROOT, "modules", Static::File(text::Global::Modules)),
    (ROOT, "kallsyms", Static::File(text::Global::Kallsyms)),
    (ROOT, "timer_list", Static::File(text::Global::TimerList)),
    (ROOT, "buddyinfo", Static::File(text::Global::Buddyinfo)),
    (ROOT, "zoneinfo", Static::File(text::Global::Zoneinfo)),
    (ROOT, "crypto", Static::File(text::Global::Crypto)),
    // Q2 seccomp sysctls; append to preserve the existing positional indices.
    (13, "seccomp", Static::Dir),
    (
        124,
        "actions_avail",
        Static::File(text::Global::SeccompActionsAvail),
    ),
    (
        124,
        "actions_logged",
        Static::File(text::Global::SeccompActionsLogged),
    ),
    (13, "acct", Static::File(text::Global::Acct)),
];

/// The file of an integer sysctl.
const fn tunable(tunable: Tunable) -> Static {
    Static::File(text::Global::Tunable(tunable))
}

/// `/proc/sys/user/max_*` for `kind`.
const fn ucount(kind: UcountKind) -> Static {
    Static::File(text::Global::Ucount(kind))
}

/// `fs.protected_*` for `which`.
const fn protected(which: Protection) -> Static {
    Static::File(text::Global::Protected(which))
}

/// What an entry of a process or thread directory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PidEntry {
    /// `fd/`, `fdinfo/`, `task/`.
    Fds,
    FdInfos,
    Tasks,
    /// `exe`, `cwd`, `root`.
    Exe,
    Cwd,
    Root,
    /// `ns/`, `net/`.
    Namespaces,
    Net,
    Attrs,
    /// `mem`: the address space, read and written at the file offset.
    Mem,
    File(text::PerPid),
}

/// The entries of `/proc/<pid>` and `/proc/<pid>/task/<tid>`: name, kind, mode. `task` and
/// `timens_offsets` only exist for the process.
const ATTR_ENTRIES: &[(&str, u32)] = &[
    ("current", 0o666),
    ("prev", 0o444),
    ("exec", 0o666),
    ("fscreate", 0o666),
    ("keycreate", 0o666),
    ("sockcreate", 0o666),
];

const PID_ENTRIES: &[(&str, PidEntry, u32)] = &[
    ("fd", PidEntry::Fds, 0o500),
    ("fdinfo", PidEntry::FdInfos, 0o555),
    ("task", PidEntry::Tasks, 0o555),
    ("exe", PidEntry::Exe, 0o777),
    ("cwd", PidEntry::Cwd, 0o777),
    ("root", PidEntry::Root, 0o777),
    ("ns", PidEntry::Namespaces, 0o511),
    ("net", PidEntry::Net, 0o555),
    ("mem", PidEntry::Mem, 0o600),
    ("cmdline", PidEntry::File(text::PerPid::Cmdline), 0o444),
    ("comm", PidEntry::File(text::PerPid::Comm), 0o644),
    ("environ", PidEntry::File(text::PerPid::Environ), 0o400),
    ("limits", PidEntry::File(text::PerPid::Limits), 0o444),
    ("maps", PidEntry::File(text::PerPid::Maps), 0o444),
    ("mountinfo", PidEntry::File(text::PerPid::Mountinfo), 0o444),
    ("mounts", PidEntry::File(text::PerPid::Mounts), 0o444),
    ("stat", PidEntry::File(text::PerPid::Stat), 0o444),
    ("statm", PidEntry::File(text::PerPid::Statm), 0o444),
    ("status", PidEntry::File(text::PerPid::Status), 0o444),
    ("uid_map", PidEntry::File(text::PerPid::UidMap), 0o644),
    ("gid_map", PidEntry::File(text::PerPid::GidMap), 0o644),
    ("projid_map", PidEntry::File(text::PerPid::ProjidMap), 0o644),
    ("setgroups", PidEntry::File(text::PerPid::Setgroups), 0o644),
    (
        "timens_offsets",
        PidEntry::File(text::PerPid::TimensOffsets),
        0o644,
    ),
    ("oom_score", PidEntry::File(text::PerPid::OomScore), 0o444),
    (
        "oom_score_adj",
        PidEntry::File(text::PerPid::OomScoreAdj),
        0o644,
    ),
    ("oom_adj", PidEntry::File(text::PerPid::OomAdj), 0o644),
    (
        "personality",
        PidEntry::File(text::PerPid::Personality),
        0o400,
    ),
    ("wchan", PidEntry::File(text::PerPid::Wchan), 0o444),
    ("stack", PidEntry::File(text::PerPid::Stack), 0o400),
    (
        "timerslack_ns",
        PidEntry::File(text::PerPid::TimerslackNs),
        0o666,
    ),
    ("schedstat", PidEntry::File(text::PerPid::Schedstat), 0o444),
    ("sched", PidEntry::File(text::PerPid::Sched), 0o644),
    ("loginuid", PidEntry::File(text::PerPid::Loginuid), 0o644),
    ("sessionid", PidEntry::File(text::PerPid::Sessionid), 0o444),
    (
        "mountstats",
        PidEntry::File(text::PerPid::Mountstats),
        0o400,
    ),
    ("smaps", PidEntry::File(text::PerPid::Smaps), 0o444),
    (
        "smaps_rollup",
        PidEntry::File(text::PerPid::SmapsRollup),
        0o444,
    ),
    ("io", PidEntry::File(text::PerPid::Io), 0o400),
    ("auxv", PidEntry::File(text::PerPid::Auxv), 0o400),
    ("timers", PidEntry::File(text::PerPid::Timers), 0o444),
    ("cgroup", PidEntry::File(text::PerPid::Cgroup), 0o444),
    (
        "coredump_filter",
        PidEntry::File(text::PerPid::CoredumpFilter),
        0o644,
    ),
    ("attr", PidEntry::Attrs, 0o555),
];

/// The files and directories of `/proc/<pid>/net`, which show the task's network namespace:
/// parent (an index here, `None` for `net` itself), name, and a file or `None` for a
/// directory.
const NET_ENTRIES: &[(Option<usize>, &str, Option<text::Net>)] = &[
    (None, "tcp", Some(text::Net::Tcp(false))),
    (None, "tcp6", Some(text::Net::Tcp(true))),
    (None, "udp", Some(text::Net::Udp(false))),
    (None, "udp6", Some(text::Net::Udp(true))),
    (None, "icmp", Some(text::Net::Icmp(false))),
    (None, "icmp6", Some(text::Net::Icmp(true))),
    (None, "raw", Some(text::Net::Raw(false))),
    (None, "raw6", Some(text::Net::Raw(true))),
    (None, "unix", Some(text::Net::Unix)),
    (None, "dev", Some(text::Net::Dev)),
    (None, "route", Some(text::Net::Route)),
    (None, "ipv6_route", Some(text::Net::Ipv6Route)),
    (None, "if_inet6", Some(text::Net::IfInet6)),
    (None, "igmp", Some(text::Net::Igmp)),
    (None, "igmp6", Some(text::Net::Igmp6)),
    (None, "mcfilter", Some(text::Net::Mcfilter(false))),
    (None, "mcfilter6", Some(text::Net::Mcfilter(true))),
    (None, "sctp", None),
    (Some(NET_SCTP), "assocs", Some(text::Net::SctpAssocs)),
    (Some(NET_SCTP), "eps", Some(text::Net::SctpEps)),
    (Some(NET_SCTP), "snmp", Some(text::Net::SctpSnmp)),
];

/// `net/sctp` (`sctp_proc_init`).
const NET_SCTP: usize = 17;

/// The entries of a `net` directory (`parent` `None`) or of one of its directories.
fn net_children(scope: Scope, parent: Option<usize>) -> Vec<(u64, Vec<u8>, Entry)> {
    NET_ENTRIES
        .iter()
        .enumerate()
        .filter(|(_, (p, _, _))| *p == parent)
        .map(|(index, (_, name, _))| {
            (
                index as u64,
                name.as_bytes().to_vec(),
                Entry::Net(scope, index),
            )
        })
        .collect()
}

/// Where pids start in the root's directory positions, after the static entries.
const FIRST_PID_POS: u64 = 1024;

pub(super) struct ProcNode {
    source: Arc<Source>,
    entry: Entry,
}

impl ProcNode {
    pub(super) fn root(source: Arc<Source>) -> Arc<dyn Node> {
        Arc::new(Self {
            source,
            entry: Entry::Root,
        })
    }

    /// Whether this is `/proc/kmsg`.
    pub(super) fn is_kmsg(&self) -> bool {
        matches!(self.entry, Entry::Static(index) if matches!(STATIC[index].2, Static::File(text::Global::Kmsg)))
    }

    /// The process of a `/proc/<pid>` directory.
    pub(super) fn tgid_dir(&self) -> Option<u32> {
        match self.entry {
            Entry::Dir(Scope { pid, tid: None }) => Some(pid),
            _ => None,
        }
    }

    fn node(&self, entry: Entry) -> Arc<dyn Node> {
        Arc::new(Self {
            source: self.source.clone(),
            entry,
        })
    }

    /// The scope of a per-process node; ESRCH-like `ENOENT` once the process is reaped.
    fn alive(&self, scope: Scope) -> FsResult<()> {
        let exists = match scope.tid {
            None => self.source.process(scope.pid).is_some(),
            Some(tid) => self.source.task(scope.pid, Some(tid)).is_some(),
        };
        if exists { Ok(()) } else { Err(Errno::ENOENT) }
    }

    /// The entries of a directory: position, name, entry.
    fn children(&self) -> FsResult<Vec<(u64, Vec<u8>, Entry)>> {
        let mut out = Vec::new();
        match self.entry {
            Entry::Root | Entry::Static(_) => {
                let me = match self.entry {
                    Entry::Static(index) => index,
                    _ => ROOT,
                };
                for (index, (parent, name, _)) in STATIC.iter().enumerate() {
                    if *parent == me {
                        out.push((index as u64, name.as_bytes().to_vec(), Entry::Static(index)));
                    }
                }
                if self.entry == Entry::Root {
                    for (nr, pid) in self.source.pids() {
                        let scope = Scope { pid, tid: None };
                        let name = nr.to_string().into_bytes();
                        out.push((FIRST_PID_POS + u64::from(nr), name, Entry::Dir(scope)));
                    }
                }
            }
            Entry::Dir(scope) => {
                self.alive(scope)?;
                for (index, (name, kind, _)) in PID_ENTRIES.iter().enumerate() {
                    // Only a process has these (`tgid_base_stuff`, not `tid_base_stuff`).
                    let process_only = matches!(
                        kind,
                        PidEntry::Tasks | PidEntry::File(text::PerPid::TimensOffsets)
                    );
                    if process_only && scope.tid.is_some() {
                        continue;
                    }
                    out.push((
                        index as u64,
                        name.as_bytes().to_vec(),
                        Entry::Pid(scope, index),
                    ));
                }
            }
            Entry::Pid(scope, index) => match PID_ENTRIES[index].1 {
                PidEntry::Tasks => {
                    for (nr, tid) in self.source.threads(scope.pid) {
                        let thread = Scope {
                            pid: scope.pid,
                            tid: Some(tid),
                        };
                        let name = nr.to_string().into_bytes();
                        out.push((u64::from(nr), name, Entry::Dir(thread)));
                    }
                }
                kind @ (PidEntry::Fds | PidEntry::FdInfos) => {
                    let task = self
                        .source
                        .task(scope.pid, scope.tid)
                        .ok_or(Errno::ENOENT)?;
                    let fds: Vec<u32> = task
                        .files
                        .lock_or_recover()
                        .iter()
                        .map(|(fd, _, _)| fd.0 as u32)
                        .collect();
                    for fd in fds {
                        let entry = match kind {
                            PidEntry::Fds => Entry::Fd(scope, fd),
                            _ => Entry::FdInfo(scope, fd),
                        };
                        out.push((u64::from(fd), fd.to_string().into_bytes(), entry));
                    }
                }
                PidEntry::Attrs => {
                    self.alive(scope)?;
                    for (index, (name, _)) in ATTR_ENTRIES.iter().enumerate() {
                        out.push((
                            index as u64,
                            name.as_bytes().to_vec(),
                            Entry::Attr(scope, index),
                        ));
                    }
                }
                PidEntry::Namespaces => {
                    self.alive(scope)?;
                    for (index, link) in NsLink::ALL.iter().enumerate() {
                        let name = link.name().as_bytes().to_vec();
                        out.push((index as u64, name, Entry::Ns(scope, index)));
                    }
                }
                // `proc_tgid_net_readdir`: nothing once the task has no namespaces.
                PidEntry::Net => {
                    self.source
                        .task(scope.pid, scope.tid)
                        .ok_or(Errno::ENOENT)?;
                    out.extend(net_children(scope, None));
                }
                _ => return Err(Errno::ENOTDIR),
            },
            Entry::Net(scope, index) if NET_ENTRIES[index].2.is_none() => {
                self.source
                    .task(scope.pid, scope.tid)
                    .ok_or(Errno::ENOENT)?;
                out.extend(net_children(scope, Some(index)));
            }
            Entry::Fd(..)
            | Entry::FdInfo(..)
            | Entry::Ns(..)
            | Entry::Net(..)
            | Entry::Attr(..) => {
                return Err(Errno::ENOTDIR);
            }
        }
        Ok(out)
    }

    /// The open file behind `fd/<n>` or `fdinfo/<n>`, with its close-on-exec flag.
    fn open_file(&self, scope: Scope, fd: u32) -> FsResult<(Arc<crate::file::OpenFile>, bool)> {
        let task = self
            .source
            .task(scope.pid, scope.tid)
            .ok_or(Errno::ENOENT)?;
        let files = task.files.lock_or_recover();
        let fd = Fd(fd as i32);
        let file = files.get(fd).ok_or(Errno::ENOENT)?;
        Ok((file, files.cloexec(fd).unwrap_or_default()))
    }

    fn mode(&self) -> u32 {
        match self.entry {
            Entry::Root | Entry::Dir(_) => 0o555,
            Entry::Static(index) => match STATIC[index].2 {
                Static::Dir => 0o555,
                Static::File(file) => file.mode(),
                Static::SysNet => 0o555,
                Static::Link(_) | Static::SelfLink | Static::ThreadSelf => 0o777,
            },
            Entry::Pid(_, index) => PID_ENTRIES[index].2,
            Entry::Net(_, index) if NET_ENTRIES[index].2.is_none() => 0o555,
            Entry::FdInfo(..) | Entry::Net(..) => 0o444,
            Entry::Ns(..) => 0o777,
            Entry::Attr(_, index) => ATTR_ENTRIES[index].1,
            Entry::Fd(scope, fd) => {
                // The link's mode shows how the file is open (`tid_fd_update_inode`).
                match self.open_file(scope, fd) {
                    Ok((file, _)) => match file.flags() & OpenFlags::O_ACCMODE {
                        OpenFlags::O_RDONLY => 0o500,
                        OpenFlags::O_WRONLY => 0o300,
                        _ => 0o700,
                    },
                    Err(_) => 0o700,
                }
            }
        }
    }

    /// Who owns the node: root for the system files, and for a process's own its effective
    /// uid and gid, or root's once it is not dumpable (`task_dump_owner`).
    fn owner(&self) -> (u32, u32) {
        let scope = match self.entry {
            Entry::Root | Entry::Static(_) => return (0, 0),
            Entry::Dir(scope)
            | Entry::Pid(scope, _)
            | Entry::Fd(scope, _)
            | Entry::FdInfo(scope, _)
            | Entry::Ns(scope, _)
            | Entry::Net(scope, _)
            | Entry::Attr(scope, _) => scope,
        };
        let Some(info) = self.source.process(scope.pid) else {
            return (0, 0);
        };
        let process = info.process;
        let cred = process.cred();
        if process.ident.lock_or_recover().dumpable != crate::ident::Dumpable::User {
            // The root of the user namespace the image runs in (`mm->user_ns`), or the global
            // root without one.
            let ns = &cred.user_ns;
            return (
                ns.root_uid().map_or(0, |uid| uid.0),
                ns.root_gid().map_or(0, |gid| gid.0),
            );
        }
        (cred.user.effective.0, cred.group.effective.0)
    }

    /// `proc_sys_permission` for a sysctl whose table root gives the permissions for the
    /// opener's namespace, from the table mode `mode`: `pid_max`'s
    /// (`pid_table_root_permissions`, the opener's pid namespace: the owner's bits for
    /// `CAP_SYS_ADMIN` over its owner or its root uid, the group's for its root gid, else the
    /// others') and `max_*_namespaces`' (`set_permissions`, the opener's user namespace: the
    /// owner's bits with `CAP_SYS_RESOURCE` over it, else the others'). Then `test_perm`.
    fn sysctl_root_permission(
        &self,
        task: u32,
        file: text::Global,
        mode: u32,
        mask: u32,
    ) -> FsResult<()> {
        let Some(cred) = self.source.cred_of(task) else {
            return Ok(());
        };
        let in_egroup = |gid| cred.group.effective == gid || cred.groups.contains(&gid);
        let bits = match file {
            text::Global::PidMax => {
                let owner = self.source.pid_ns_of(task).user_ns().clone();
                if cred.ns_capable(&owner, Cap::SysAdmin)
                    || owner.root_uid() == Some(cred.user.effective)
                {
                    (mode >> 6) & 7
                } else if owner.root_gid().is_some_and(in_egroup) {
                    (mode >> 3) & 7
                } else {
                    mode & 4
                }
            }
            _ => match cred.ns_capable(&self.source.user_ns_of(task), Cap::SysResource) {
                true => (mode >> 6) & 7,
                false => mode & 4,
            },
        };
        test_perm(&cred, (bits << 6) | (bits << 3) | bits, mask)
    }

    /// `proc_ns_get_link`: the namespace's nsfs inode, for a caller that may read the task's
    /// state (`PTRACE_MODE_READ_FSCREDS`, `EACCES`).
    fn ns_link(&self, cx: &Ctx<'_>, scope: Scope, link: NsLink) -> FsResult<Link> {
        let target = self.source.process(scope.pid).ok_or(Errno::ENOENT)?;
        let task = self
            .source
            .task(scope.pid, scope.tid)
            .ok_or(Errno::ENOENT)?;
        let caller = self
            .source
            .tgid_of(cx.task)
            .and_then(|tgid| self.source.process(tgid))
            .ok_or(Errno::EACCES)?;
        if !Arc::ptr_eq(&caller.process, &target.process) {
            let dumpable =
                target.process.ident.lock_or_recover().dumpable == crate::ident::Dumpable::User;
            let cred = caller.process.cred();
            if !cred.may_read_task(&target.process.cred(), dumpable, true) {
                return Err(Errno::EACCES);
            }
        }
        // `ns_get_path`: a task that is done has none.
        let task_ns = task.ns.as_ref().ok_or(Errno::ENOENT)?;
        let ns = link.get(
            task_ns,
            &target.process.pid_ns,
            &target.process.cred().user_ns,
        );
        Ok(Link::Jump(self.source.nsfs.path(&ns)))
    }
}

/// `test_perm` (`fs/proc/proc_sysctl.c`): the owner's bits of a sysctl's `mode` for the
/// initial namespace's root, the group's for its group, else the others'; never execution.
pub(super) fn test_perm(cred: &Cred, mode: u32, mask: u32) -> FsResult<()> {
    let mask = mask & (wasixv2_vfs::MAY_READ | wasixv2_vfs::MAY_WRITE | wasixv2_vfs::MAY_EXEC);
    let bits = if cred.user.effective == Kuid(0) {
        mode >> 6
    } else if cred.group.effective == Kgid(0) || cred.groups.contains(&Kgid(0)) {
        mode >> 3
    } else {
        mode
    } & 7;
    match mask & !bits {
        0 => Ok(()),
        _ => Err(Errno::EACCES),
    }
}

/// Inode numbers. Static nodes count from 1; a process or thread directory and everything in
/// it carry the pid or tid above bit 30, with the entry below.
fn ino(entry: Entry) -> u64 {
    let scoped = |scope: Scope, sub: u64| {
        let key = match scope.tid {
            None => u64::from(scope.pid),
            Some(tid) => (1 << 31) | u64::from(tid),
        };
        (1 << 63) | (key << 30) | sub
    };
    match entry {
        Entry::Root => 1,
        Entry::Static(index) if matches!(STATIC[index].2, Static::SysNet) => {
            super::sysnet::ROOT_INO
        }
        Entry::Static(index) => 2 + index as u64,
        Entry::Dir(scope) => scoped(scope, 0),
        Entry::Pid(scope, index) => scoped(scope, 1 + index as u64),
        Entry::Fd(scope, fd) => scoped(scope, (1 << 29) | u64::from(fd)),
        Entry::FdInfo(scope, fd) => scoped(scope, (1 << 28) | u64::from(fd)),
        Entry::Ns(scope, index) => scoped(scope, (1 << 27) | index as u64),
        Entry::Net(scope, index) => scoped(scope, (1 << 26) | index as u64),
        Entry::Attr(scope, index) => scoped(scope, (1 << 25) | index as u64),
    }
}

fn file_type(entry: Entry) -> FileType {
    match entry {
        Entry::Root | Entry::Dir(_) => FileType::Directory,
        Entry::Static(index) => match STATIC[index].2 {
            Static::Dir => FileType::Directory,
            Static::File(_) => FileType::Regular,
            Static::SysNet => FileType::Directory,
            Static::Link(_) | Static::SelfLink | Static::ThreadSelf => FileType::Symlink,
        },
        Entry::Pid(_, index) => match PID_ENTRIES[index].1 {
            PidEntry::Fds
            | PidEntry::FdInfos
            | PidEntry::Tasks
            | PidEntry::Namespaces
            | PidEntry::Net
            | PidEntry::Attrs => FileType::Directory,
            PidEntry::Exe | PidEntry::Cwd | PidEntry::Root => FileType::Symlink,
            PidEntry::File(_) | PidEntry::Mem => FileType::Regular,
        },
        Entry::Fd(..) | Entry::Ns(..) => FileType::Symlink,
        Entry::Net(_, index) if NET_ENTRIES[index].2.is_none() => FileType::Directory,
        Entry::FdInfo(..) | Entry::Net(..) | Entry::Attr(..) => FileType::Regular,
    }
}

#[async_trait(?Send)]
impl Node for ProcNode {
    fn ino(&self) -> u64 {
        ino(self.entry)
    }

    fn file_type(&self) -> FileType {
        file_type(self.entry)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// `proc_sys_permission` for the sysctls whose table root decides for the opener's
    /// namespace (`pid_max`, `max_*_namespaces`); the mode bits for everything else.
    /// `/proc/sys/net` has its own (`SysNetNode`).
    fn permission(
        &self,
        cred: &wasixv2_vfs::Cred,
        task: u32,
        attr: &Attr,
        acl: Option<&wasixv2_vfs::PosixAcl>,
        mask: u32,
    ) -> FsResult<()> {
        match self.entry {
            Entry::Static(index) => match STATIC[index].2 {
                Static::File(file @ (text::Global::PidMax | text::Global::Ucount(_))) => {
                    self.sysctl_root_permission(task, file, attr.mode, mask)
                }
                _ => wasixv2_vfs::generic_permission_acl(cred, attr, acl, mask),
            },
            _ => wasixv2_vfs::generic_permission_acl(cred, attr, acl, mask),
        }
    }

    /// `proc_splice_unmountable`: nothing is mounted on a descriptor's link or its `fdinfo`.
    fn unmountable(&self) -> bool {
        matches!(self.entry, Entry::Fd(..) | Entry::FdInfo(..))
    }

    async fn getattr(&self) -> FsResult<Attr> {
        let file_type = self.file_type();
        let (uid, gid) = self.owner();
        let time = self.source.mounted;
        Ok(Attr {
            ino: self.ino(),
            file_type,
            mode: self.mode(),
            nlink: if file_type == FileType::Directory {
                2
            } else {
                1
            },
            uid,
            gid,
            rdev: Default::default(),
            size: 0,
            blocks: 0,
            blksize: 1024,
            atime: time,
            mtime: time,
            ctime: time,
            btime: None,
            dev: None,
        })
    }

    /// `proc_setattr` and `proc_sys_setattr`: modes and owners are the kernel's (`EPERM`).
    /// Anything else is accepted and changes nothing a reader sees, so opening a file with
    /// `O_TRUNC`, as `fopen(path, "w")` and a shell's `>` do, succeeds.
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        if attr.mode.is_some() || attr.uid.is_some() || attr.gid.is_some() {
            return Err(Errno::EPERM);
        }
        self.getattr().await
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if self.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        // Process and descriptor directories are looked up by number, not by listing them;
        // pids are the instance's pid namespace's.
        let number = std::str::from_utf8(name)
            .ok()
            .filter(|s| !s.starts_with('0') || *s == "0")
            .and_then(|s| s.parse::<u32>().ok());
        match (self.entry, number) {
            (Entry::Root, Some(nr)) => {
                let pid = self.source.find(nr).ok_or(Errno::ENOENT)?;
                let scope = Scope { pid, tid: None };
                self.alive(scope)?;
                return Ok(self.node(Entry::Dir(scope)));
            }
            (Entry::Pid(scope, index), Some(n)) => {
                let entry = match PID_ENTRIES[index].1 {
                    PidEntry::Tasks => {
                        let tid = self.source.find(n).ok_or(Errno::ENOENT)?;
                        let thread = Scope {
                            pid: scope.pid,
                            tid: Some(tid),
                        };
                        self.alive(thread)?;
                        Entry::Dir(thread)
                    }
                    PidEntry::Fds => Entry::Fd(scope, n),
                    PidEntry::FdInfos => Entry::FdInfo(scope, n),
                    _ => return Err(Errno::ENOENT),
                };
                if matches!(entry, Entry::Fd(..) | Entry::FdInfo(..)) {
                    self.open_file(scope, n)?;
                }
                return Ok(self.node(entry));
            }
            _ => {}
        }
        let found = self
            .children()?
            .into_iter()
            .find(|(_, child, _)| child == name)
            .ok_or(Errno::ENOENT)?;
        if let Entry::Static(index) = found.2
            && let Static::SysNet = STATIC[index].2
        {
            return Ok(super::sysnet::SysNetNode::root(self.source.clone()));
        }
        Ok(self.node(found.2))
    }

    async fn readlink(&self) -> FsResult<Vec<u8>> {
        match self.entry {
            Entry::Static(index) => match STATIC[index].2 {
                Static::Link(body) => Ok(body.to_vec()),
                _ => Err(Errno::EINVAL),
            },
            _ => Err(Errno::EINVAL),
        }
    }

    /// `proc_self_get_link`, `proc_thread_self_get_link`, and the magic links
    /// (`proc_fd_link`, `proc_exe_link`, `proc_cwd_link`, `proc_root_link`).
    async fn get_link(&self, cx: &Ctx<'_>) -> FsResult<Link> {
        let entry = match self.entry {
            Entry::Static(index) => match STATIC[index].2 {
                Static::Link(body) => return Ok(Link::Body(body.to_vec())),
                // The caller as the instance's pid namespace sees it: nothing for one outside
                // it (`proc_self_get_link`).
                Static::SelfLink => {
                    let tgid = self.source.tgid_of(cx.task).ok_or(Errno::ENOENT)?;
                    let tgid = Some(self.source.nr(tgid)).filter(|&nr| nr != 0);
                    let tgid = tgid.ok_or(Errno::ENOENT)?;
                    return Ok(Link::Body(tgid.to_string().into_bytes()));
                }
                Static::ThreadSelf => {
                    let tgid = self.source.tgid_of(cx.task).ok_or(Errno::ENOENT)?;
                    let (tgid, tid) = (self.source.nr(tgid), self.source.nr(cx.task));
                    if tgid == 0 || tid == 0 {
                        return Err(Errno::ENOENT);
                    }
                    let body = format!("{tgid}/task/{tid}");
                    return Ok(Link::Body(body.into_bytes()));
                }
                _ => return Err(Errno::EINVAL),
            },
            Entry::Fd(scope, fd) => {
                let (file, _) = self.open_file(scope, fd)?;
                return Ok(Link::Jump(file.path().clone()));
            }
            Entry::Ns(scope, index) => return self.ns_link(cx, scope, NsLink::ALL[index]),
            Entry::Pid(scope, index) => (scope, PID_ENTRIES[index].1),
            _ => return Err(Errno::EINVAL),
        };
        let (scope, kind) = entry;
        let path = match kind {
            PidEntry::Exe => {
                let process = self.source.process(scope.pid).ok_or(Errno::ENOENT)?;
                process.process.exe.lock_or_recover().clone()
            }
            PidEntry::Cwd | PidEntry::Root => {
                let task = self.source.task(scope.pid, scope.tid);
                task.map(|task| {
                    let fs = task.fs.lock_or_recover();
                    match kind {
                        PidEntry::Cwd => fs.cwd.clone(),
                        _ => fs.root.clone(),
                    }
                })
            }
            _ => return Err(Errno::EINVAL),
        };
        path.map(Link::Jump).ok_or(Errno::ENOENT)
    }

    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        if self.file_type() == FileType::Directory {
            self.children()?;
            return Ok(Box::new(DirHandle(self.node(self.entry))));
        }
        if let Entry::Pid(scope, index) = self.entry
            && PID_ENTRIES[index].1 == PidEntry::Mem
        {
            return self.open_mem(scope, options.task);
        }
        if let Entry::Attr(scope, _) = self.entry {
            self.alive(scope)?;
            let opener_mm = self.source.state.upgrade().and_then(|state| {
                state
                    .read_or_recover()
                    .tasks
                    .get(&wasixv2_syscalls::TaskId(options.task))
                    .map(|task| task.mm.clone())
            });
            return Ok(Box::new(AttrHandle {
                source: self.source.clone(),
                scope,
                opener: options.task,
                opener_mm,
            }));
        }
        let (data, comm) = match self.entry {
            Entry::Static(index) => match STATIC[index].2 {
                Static::File(file) => {
                    let ns = self.source.ns_of(options.task);
                    let pid_ns = self.source.pid_ns_of(options.task);
                    let opener = self.source.cred_of(options.task);
                    let user_ns = self.source.user_ns_of(options.task);
                    let data = match file {
                        // What the opener may view, as its keyrings make it possess keys.
                        text::Global::Keys => self.source.proc_keys(options.task, &user_ns),
                        text::Global::KeyUsers => self.source.keys.proc_key_users(&user_ns),
                        _ => text::global(&self.source, &ns, &pid_ns, &user_ns, file)?,
                    };
                    let write = match file {
                        _ if !options.write => None,
                        // `pid_table_root_permissions`: `CAP_SYS_ADMIN` over the pid
                        // namespace's owner, or being its root.
                        text::Global::PidMax => {
                            let owner = pid_ns.user_ns();
                            let may = opener.as_ref().is_none_or(|cred| {
                                cred.ns_capable(owner, Cap::SysAdmin)
                                    || owner.root_uid() == Some(cred.user.effective)
                            });
                            if !may {
                                return Err(Errno::EACCES);
                            }
                            Some(Write::PidMax(pid_ns))
                        }
                        // `set_permissions`: `CAP_SYS_RESOURCE` over the opener's own user
                        // namespace, whose limits these are.
                        text::Global::Ucount(kind) => {
                            if !opener.is_none_or(|cred| cred.capable(Cap::SysResource)) {
                                return Err(Errno::EACCES);
                            }
                            Some(Write::Ucount(user_ns, kind))
                        }
                        text::Global::NsLastPid => {
                            let may = self.source.may_checkpoint_restore(options.task, &pid_ns);
                            Some(Write::NsLastPid(pid_ns, may))
                        }
                        text::Global::NrOpen => Some(Write::NrOpen(self.source.nr_open.clone())),
                        text::Global::DirNotifyEnable
                        | text::Global::LeasesEnable
                        | text::Global::LeaseBreakTime => {
                            Some(Write::Fs(self.source.sysctls.clone(), file))
                        }
                        text::Global::AioMaxNr => Some(Write::AioMaxNr(self.source.aio.clone())),
                        text::Global::PipeMaxSize => {
                            Some(Write::PipeMaxSize(self.source.pipe_max_size.clone()))
                        }
                        text::Global::Overcommit
                        | text::Global::OvercommitRatio
                        | text::Global::OvercommitKbytes
                        | text::Global::MaxMapCount => {
                            Some(Write::Vm(self.source.vm.clone(), file))
                        }
                        text::Global::Tunable(tunable) => {
                            Some(Write::Tunable(self.source.tunables.clone(), tunable))
                        }
                        text::Global::SysrqTrigger => Some(Write::Sysrq(self.source.clone())),
                        text::Global::DropCaches => {
                            Some(Write::DropCaches(self.source.clone(), options.task))
                        }
                        // `proc_dostring_coredump` and `proc_dointvec`: root's, by the mode.
                        text::Global::CorePattern
                        | text::Global::CoreUsesPid
                        | text::Global::CorePipeLimit => {
                            Some(Write::Core(self.source.coredump.clone(), file))
                        }
                        text::Global::Acct => Some(Write::Acct(self.source.acct.clone())),
                        text::Global::SeccompActionsLogged => {
                            Some(Write::Seccomp(self.source.clone(), options.task))
                        }
                        text::Global::Printk | text::Global::PrintkDevkmsg => {
                            Some(Write::Printk(self.source.printk.clone(), file, true))
                        }
                        // `proc_dointvec_minmax_sysadmin`: `capable(CAP_SYS_ADMIN)`.
                        text::Global::DmesgRestrict => {
                            let may = opener
                                .as_ref()
                                .is_none_or(|cred| cred.capable_init(Cap::SysAdmin));
                            Some(Write::Printk(self.source.printk.clone(), file, may))
                        }
                        // The initial namespace's limits, root's by the mode.
                        text::Global::InitUcount(kind) => {
                            let mut init = user_ns.clone();
                            while let Some(parent) = init.parent().cloned() {
                                init = parent;
                            }
                            Some(Write::Ucount(init, kind))
                        }
                        text::Global::Protected(which) => {
                            Some(Write::Protected(self.source.vfs.clone(), which))
                        }
                        text::Global::Pressure => Some(Write::Pressure),
                        // `proc_taint`: `capable(CAP_SYS_ADMIN)`, of the opener here.
                        text::Global::Tainted => {
                            let may = opener.is_none_or(|cred| cred.capable_init(Cap::SysAdmin));
                            Some(Write::Taint(self.source.taint.clone(), may))
                        }
                        // `yama_dointvec_minmax`: `capable(CAP_SYS_PTRACE)`, of the opener here.
                        text::Global::PtraceScope => {
                            let may = opener.is_none_or(|cred| cred.capable_init(Cap::SysPtrace));
                            Some(Write::Yama(self.source.yama.clone(), may))
                        }
                        _ => None,
                    };
                    (data, write)
                }
                _ => return Err(Errno::ENXIO),
            },
            Entry::Pid(scope, index) => match PID_ENTRIES[index].1 {
                PidEntry::File(file) => {
                    self.check_pid_file(scope, file, options.task)?;
                    let reader = self.source.user_ns_of(options.task);
                    let viewer = self.source.ns_of(options.task);
                    let data = text::per_pid(&self.source, scope, file, &reader, &viewer)?;
                    let write = match options.write {
                        true => self.per_pid_write(scope, file, options.task)?,
                        false => None,
                    };
                    (data, write)
                }
                _ => return Err(Errno::ENXIO),
            },
            Entry::FdInfo(scope, fd) => {
                let (file, cloexec) = self.open_file(scope, fd)?;
                (text::fdinfo(&file, cloexec), None)
            }
            Entry::Net(scope, index) => {
                let task = self
                    .source
                    .task(scope.pid, scope.tid)
                    .ok_or(Errno::ENOENT)?;
                let ns = task.ns.as_ref().ok_or(Errno::ENOENT)?;
                let file = NET_ENTRIES[index].2.ok_or(Errno::EISDIR)?;
                (text::net(&ns.net, file), None)
            }
            _ => return Err(Errno::ENXIO),
        };
        let seccomp = match self.entry {
            Entry::Static(index)
                if matches!(
                    STATIC[index].2,
                    Static::File(text::Global::SeccompActionsLogged)
                ) =>
            {
                Some(self.source.seccomp_logging.clone())
            }
            _ => None,
        };
        Ok(Box::new(FileHandle {
            data,
            write: comm,
            seccomp,
        }))
    }
}

impl ProcNode {
    /// What a write to the file `file` of a process's directory, which the task `opener`
    /// opens to write, changes. The id maps are of the process's user namespace as the file
    /// opens (`proc_id_map_open`), written with the opener's credentials (`f_cred`); opening
    /// `setgroups` to write needs `CAP_SYS_ADMIN` over that namespace (`proc_setgroups_open`).
    fn per_pid_write(
        &self,
        scope: Scope,
        file: text::PerPid,
        opener: u32,
    ) -> FsResult<Option<Write>> {
        let map = match file {
            text::PerPid::Comm => return Ok(Some(Write::Comm(self.source.clone(), scope))),
            // Written with the opener's credentials (`file_ns_capable`).
            text::PerPid::TimensOffsets => {
                let cred = self.source.cred_of(opener).ok_or(Errno::EPERM)?;
                let target = (self.source.clone(), scope);
                return Ok(Some(Write::TimensOffsets(target, Box::new(cred))));
            }
            text::PerPid::CoredumpFilter => {
                let info = self.source.process(scope.pid).ok_or(Errno::ESRCH)?;
                return Ok(Some(Write::CoredumpFilter(info.process)));
            }
            text::PerPid::UidMap => MapKind::Uid,
            text::PerPid::GidMap => MapKind::Gid,
            text::PerPid::ProjidMap => MapKind::Projid,
            text::PerPid::Setgroups => {
                let target = self.user_ns(scope)?;
                let cred = self.source.cred_of(opener).ok_or(Errno::EACCES)?;
                if !cred.ns_capable(&target, Cap::SysAdmin) {
                    return Err(Errno::EACCES);
                }
                return Ok(Some(Write::Setgroups(target)));
            }
            file @ (text::PerPid::OomScoreAdj | text::PerPid::OomAdj) => {
                let info = self.source.process(scope.pid).ok_or(Errno::ESRCH)?;
                let capable = self
                    .source
                    .cred_of(opener)
                    .is_none_or(|cred| cred.capable_init(Cap::SysResource));
                let legacy = file == text::PerPid::OomAdj;
                return Ok(Some(Write::OomAdj(info.process, legacy, capable)));
            }
            // `proc_loginuid_write`: only the task itself.
            text::PerPid::Loginuid => {
                if opener != scope.tid.unwrap_or(scope.pid) {
                    return Err(Errno::EPERM);
                }
                let info = self.source.process(scope.pid).ok_or(Errno::ESRCH)?;
                let cred = self.source.cred_of(opener).ok_or(Errno::EPERM)?;
                let sessions = self.source.audit_sessions.clone();
                return Ok(Some(Write::Loginuid(
                    info.process,
                    Box::new(cred),
                    sessions,
                )));
            }
            text::PerPid::TimerslackNs => {
                return Ok(Some(Write::Timerslack(self.source.clone(), scope)));
            }
            // `proc_sched_write`: resets statistics this kernel does not keep.
            text::PerPid::Sched => return Ok(Some(Write::Discard)),
            _ => return Ok(None),
        };
        let target = self.user_ns(scope)?;
        let cred = self.source.cred_of(opener).ok_or(Errno::EPERM)?;
        Ok(Some(Write::Map(target, map, Box::new(cred))))
    }

    /// The checks `/proc/<pid>` files make of their reader: `PTRACE_MODE_READ_FSCREDS` for `io`,
    /// `auxv` and `smaps*` (`EACCES`); attaching for `personality` (`lock_trace`, `EPERM`);
    /// `CAP_SYS_ADMIN` and attaching for `stack`; `CAP_SYS_NICE` over another task's user
    /// namespace for `timerslack_ns` (`EPERM`).
    fn check_pid_file(&self, scope: Scope, file: text::PerPid, opener: u32) -> FsResult<()> {
        use crate::ptrace::{MODE_ATTACH_FSCREDS, MODE_READ_FSCREDS};
        use text::PerPid as P;
        let may = |mode: u32| self.may_ptrace(scope, opener, mode);
        match file {
            P::Io | P::Auxv | P::Smaps | P::SmapsRollup if !may(MODE_READ_FSCREDS) => {
                Err(Errno::EACCES)
            }
            P::Personality if !may(MODE_ATTACH_FSCREDS) => Err(Errno::EPERM),
            P::Stack => {
                let admin = self
                    .source
                    .cred_of(opener)
                    .is_none_or(|cred| cred.capable_init(Cap::SysAdmin));
                match (admin, may(MODE_ATTACH_FSCREDS)) {
                    (false, _) => Err(Errno::EACCES),
                    (true, false) => Err(Errno::EPERM),
                    (true, true) => Ok(()),
                }
            }
            P::TimerslackNs if opener != scope.tid.unwrap_or(scope.pid) => {
                let target = self.user_ns(scope)?;
                let nice = self
                    .source
                    .cred_of(opener)
                    .is_none_or(|cred| cred.ns_capable(&target, Cap::SysNice));
                match nice {
                    true => Ok(()),
                    false => Err(Errno::EPERM),
                }
            }
            _ => Ok(()),
        }
    }

    /// `ptrace_may_access` of the opener `opener` to the process of `scope`; the kernel's own
    /// opens (no task) may.
    fn may_ptrace(&self, scope: Scope, opener: u32, mode: u32) -> bool {
        let (Some(target), Some(tree)) =
            (self.source.process(scope.pid), self.source.tree.upgrade())
        else {
            return false;
        };
        let opener = self
            .source
            .tgid_of(opener)
            .and_then(|tgid| self.source.process(tgid));
        opener.is_none_or(|opener| {
            crate::ptrace::may_access(
                &self.source.yama,
                &tree,
                &opener.process,
                &target.process,
                mode,
            )
        })
    }

    /// The user namespace of the process whose directory `scope` is.
    fn user_ns(&self, scope: Scope) -> FsResult<Arc<UserNs>> {
        let info = self.source.process(scope.pid).ok_or(Errno::ESRCH)?;
        Ok(info.process.cred().user_ns)
    }
}

/// `mem_open` (`proc_mem_open(inode, PTRACE_MODE_ATTACH)`): the opener needs to be allowed
/// to attach to the task (`PTRACE_MODE_ATTACH_FSCREDS`, else `EACCES`); the file holds the
/// task's address space as it was at the open.
impl ProcNode {
    fn open_mem(&self, scope: Scope, opener: u32) -> FsResult<Box<dyn Handle>> {
        let target = self.source.process(scope.pid).ok_or(Errno::ENOENT)?;
        let task = self
            .source
            .task(scope.pid, scope.tid)
            .ok_or(Errno::ENOENT)?;
        let opener = self
            .source
            .tgid_of(opener)
            .and_then(|tgid| self.source.process(tgid));
        // The kernel's own opens (no task) pass.
        if let Some(opener) = opener {
            let tree = self.source.tree.upgrade().ok_or(Errno::ENOENT)?;
            let mode = crate::ptrace::MODE_ATTACH_FSCREDS;
            let yama = &self.source.yama;
            if !crate::ptrace::may_access(yama, &tree, &opener.process, &target.process, mode) {
                return Err(Errno::EACCES);
            }
        }
        Ok(Box::new(MemHandle { mm: task.mm }))
    }
}

/// An open `/proc/<pid>/mem` (`mem_rw`): the offset is the address; each access is forced
/// (`FOLL_FORCE`, `proc_mem_force_override`'s default). Nothing is read once the address
/// space's last user left it (`mmget_not_zero`); an access that copies nothing is `EIO`.
struct MemHandle {
    mm: Arc<crate::mm::Mm>,
}

impl MemHandle {
    async fn access(
        &self,
        offset: u64,
        buf: crate::ptrace::Access<'_>,
        len: usize,
    ) -> FsResult<usize> {
        if len == 0 || self.mm.foreign().is_none() {
            return Ok(0);
        }
        let addr = u32::try_from(offset).map_err(|_| Errno::EIO)?;
        match crate::ptrace::access_remote_vm(&self.mm, addr, buf, true).await {
            0 => Err(Errno::EIO),
            n => Ok(n),
        }
    }
}

#[async_trait(?Send)]
impl Handle for MemHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let len = buf.len();
        self.access(offset, crate::ptrace::Access::Read(buf), len)
            .await
    }

    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        self.access(offset, crate::ptrace::Access::Write(data), data.len())
            .await
    }

    async fn append(&self, _: &[u8]) -> FsResult<(u64, usize)> {
        Err(Errno::EIO)
    }
}

/// An open directory, listed afresh on every read as `proc_readdir` does.
struct DirHandle(Arc<dyn Node>);

#[async_trait(?Send)]
impl Handle for DirHandle {
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let node: &ProcNode = self.0.as_any().downcast_ref().expect("a proc directory");
        let children = node.children()?;
        for (pos, name, entry) in children.iter().filter(|(pos, ..)| *pos >= cookie) {
            let entry = DirEntry {
                name,
                ino: ino(*entry),
                file_type: file_type(*entry),
                next: pos + 1,
            };
            if !sink.push(entry) {
                break;
            }
        }
        Ok(())
    }
}

/// Linux fs/proc/base.c:proc_pid_attr_read/write, with no LSM providing labels.
struct AttrHandle {
    source: Arc<Source>,
    scope: Scope,
    opener: u32,
    opener_mm: Option<Arc<crate::mm::Mm>>,
}

#[async_trait(?Send)]
impl Handle for AttrHandle {
    async fn read_at(&self, _: &mut [u8], _: u64) -> FsResult<usize> {
        self.source
            .task(self.scope.pid, self.scope.tid)
            .ok_or(Errno::ESRCH)?;
        Err(Errno::EINVAL)
    }

    async fn write_at(&self, _: &[u8], _: u64) -> FsResult<usize> {
        let current = wasixv2_syscalls::current_task().map_or(self.opener, |id| id.0);
        let state = self.source.state.upgrade().ok_or(Errno::ESRCH)?;
        let state = state.read_or_recover();
        let task = state
            .tasks
            .get(&wasixv2_syscalls::TaskId(current))
            .ok_or(Errno::ESRCH)?;
        if !self
            .opener_mm
            .as_ref()
            .is_some_and(|mm| Arc::ptr_eq(mm, &task.mm))
        {
            return Err(Errno::EPERM);
        }
        let target = self.scope.tid.unwrap_or(self.scope.pid);
        if !state.tasks.contains_key(&wasixv2_syscalls::TaskId(target)) {
            return Err(Errno::ESRCH);
        }
        if current != target {
            return Err(Errno::EACCES);
        }
        Err(Errno::EINVAL)
    }
}

/// An open file: its contents as they were at open, and what a write to it changes.
struct FileHandle {
    data: Vec<u8>,
    seccomp: Option<Arc<crate::seccomp::Logging>>,
    write: Option<Write>,
}

/// What a writable file sets.
enum Write {
    Acct(Arc<crate::acct::Acct>),
    Seccomp(Arc<Source>, u32),
    /// A task's name.
    Comm(Arc<Source>, Scope),
    /// `kernel.pid_max` of the opener's pid namespace.
    PidMax(Arc<PidNs>),
    /// `kernel.ns_last_pid` of the opener's pid namespace, if it may
    /// (`checkpoint_restore_ns_capable`).
    NsLastPid(Arc<PidNs>, bool),
    /// `fs.nr_open`.
    NrOpen(Arc<NrOpen>),
    /// `fs.pipe-max-size`.
    PipeMaxSize(Arc<PipeMaxSize>),
    /// `fs.aio-max-nr`.
    AioMaxNr(Arc<AioSysctl>),
    /// One of the `vm` tunables.
    Vm(Arc<VmSysctls>, text::Global),
    /// `kernel.tainted`, if the opener may add flags.
    Taint(Arc<Taint>, bool),
    /// `kernel.yama.ptrace_scope`, if the opener may change it.
    Yama(Arc<crate::yama::Yama>, bool),
    /// A map of a user namespace, written with the opener's credentials.
    Map(Arc<UserNs>, MapKind, Box<Cred>),
    /// Whether a user namespace allows `setgroups`.
    Setgroups(Arc<UserNs>),
    /// `user.max_*` of the opener's user namespace, or `fs.inotify.max_user_*` of the initial
    /// one.
    Ucount(Arc<UserNs>, UcountKind),
    /// One of `fs.protected_*`.
    Protected(std::sync::Weak<wasixv2_vfs::Vfs>, Protection),
    /// `/proc/pressure/*`, whose triggers (`psi_trigger_create`) are not kept.
    Pressure,
    /// One of the machine's `fs.*` integers.
    Fs(Arc<crate::fs::FsSysctls>, text::Global),
    /// The offsets of a process's time namespace for children, written with the opener's
    /// credentials.
    TimensOffsets((Arc<Source>, Scope), Box<Cred>),
    /// `oom_score_adj`, or with `true` the legacy `oom_adj`, of a process, by an opener with
    /// `CAP_SYS_RESOURCE` or not.
    OomAdj(Arc<crate::Process>, bool, bool),
    /// The audit login uid of a process, set by itself with its credentials.
    Loginuid(
        Arc<crate::Process>,
        Box<Cred>,
        Arc<std::sync::atomic::AtomicU32>,
    ),
    /// A task's `timer_slack_ns`.
    Timerslack(Arc<Source>, Scope),
    /// A write that is taken and changes nothing.
    Discard,
    /// One of the kernel log's tunables, if the opener may set it.
    Printk(Arc<crate::printk::Printk>, text::Global, bool),
    /// One of the core dump tunables.
    Core(Arc<crate::coredump::Coredump>, text::Global),
    /// A process's `coredump_filter`.
    CoredumpFilter(Arc<crate::Process>),
    /// One of the integer sysctls.
    Tunable(Arc<crate::sysctl::Tunables>, Tunable),
    /// `/proc/sysrq-trigger`.
    Sysrq(Arc<Source>),
    /// `vm.drop_caches`, by the task that opened it.
    DropCaches(Arc<Source>, u32),
}

/// `__set_oom_adj`: `oom_score_adj` from -1000 to 1000, or with `legacy` `oom_adj` from -17 to
/// 15 on its scale; lowering it below its floor (the current value for `oom_adj`) needs
/// `CAP_SYS_RESOURCE`, whose writes set the floor (`oom_score_adj_min`).
fn set_oom_adj(process: &crate::Process, value: i32, legacy: bool, capable: bool) -> FsResult<()> {
    use super::introspect::{OOM_ADJUST_MAX, OOM_DISABLE, OOM_SCORE_ADJ_MAX, OOM_SCORE_ADJ_MIN};
    let adj = match legacy {
        true if !(OOM_DISABLE..=OOM_ADJUST_MAX).contains(&value) => return Err(Errno::EINVAL),
        true if value == OOM_ADJUST_MAX => OOM_SCORE_ADJ_MAX,
        true => value * OOM_SCORE_ADJ_MAX / -OOM_DISABLE,
        false if !(OOM_SCORE_ADJ_MIN..=OOM_SCORE_ADJ_MAX).contains(&value) => {
            return Err(Errno::EINVAL);
        }
        false => value,
    };
    let mut ident = process.ident.lock_or_recover();
    let floor = match legacy {
        true => ident.oom_score_adj,
        false => ident.oom_score_adj_min,
    };
    if adj < floor && !capable {
        return Err(Errno::EACCES);
    }
    ident.oom_score_adj = adj;
    if !legacy && capable {
        ident.oom_score_adj_min = adj;
    }
    Ok(())
}

/// `audit_set_loginuid`: anyone may set an unset login uid; changing one needs
/// `CAP_AUDIT_CONTROL`. A uid starts a new session; `AUDIT_UID_UNSET` (`u32::MAX`) clears both.
fn set_loginuid(
    process: &crate::Process,
    cred: &Cred,
    sessions: &std::sync::atomic::AtomicU32,
    uid: u32,
) -> FsResult<()> {
    use std::sync::atomic::Ordering::Relaxed;
    let kuid = match uid {
        u32::MAX => None,
        uid => Some(cred.user_ns.make_kuid(uid).ok_or(Errno::EINVAL)?),
    };
    let mut ident = process.ident.lock_or_recover();
    if ident.loginuid.is_some() && !cred.capable_init(Cap::AuditControl) {
        return Err(Errno::EPERM);
    }
    ident.sessionid = match kuid {
        Some(_) => match sessions.fetch_add(1, Relaxed).wrapping_add(1) {
            // `AUDIT_SID_UNSET` is skipped.
            u32::MAX => sessions.fetch_add(1, Relaxed).wrapping_add(1),
            id => id,
        },
        None => u32::MAX,
    };
    ident.loginuid = kuid;
    Ok(())
}

/// `timens_offsets_write`'s parsing: at most two lines of `<clock> <sec> <nsec>`, the clock
/// by name or number. Returns the offsets and the bytes taken.
fn parse_timens_offsets(data: &[u8]) -> Result<(Vec<TimensOffset>, usize), Errno> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    let mut offsets = Vec::new();
    let mut taken = data.len();
    let mut rest = text;
    while !rest.is_empty() {
        let (line, next) = match rest.split_once('\n') {
            Some((line, next)) => (line, next),
            None => (rest, ""),
        };
        let mut fields = line.split_ascii_whitespace();
        let (Some(clock), Some(sec), Some(nsec)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(Errno::EINVAL);
        };
        let boottime = match clock {
            "monotonic" | "1" => false,
            "boottime" | "7" => true,
            _ => return Err(Errno::EINVAL),
        };
        let sec: i64 = sec.parse().map_err(|_| Errno::EINVAL)?;
        // `%lu`: a negative count is a huge one.
        let nsec: i64 = match nsec.parse::<u64>() {
            Ok(nsec) if nsec < 1_000_000_000 => nsec as i64,
            _ => return Err(Errno::EINVAL),
        };
        offsets.push(TimensOffset {
            boottime,
            sec,
            nsec,
        });
        if offsets.len() == 2 {
            if !next.is_empty() {
                taken = data.len() - next.len();
            }
            break;
        }
        rest = next;
    }
    Ok((offsets, taken))
}

/// `proc_dointvec`'s parsing of one integer: blanks around it, nothing else.
fn sysctl_int(data: &[u8]) -> Result<u32, Errno> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    text.trim().parse().map_err(|_| Errno::EINVAL)
}

/// [`sysctl_int`] for a signed `int`.
fn sysctl_i32(data: &[u8]) -> Result<i32, Errno> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    text.trim().parse().map_err(|_| Errno::EINVAL)
}

/// Sets one of the kernel log's tunables: `kernel.printk` takes up to four levels
/// (`proc_dointvec`), `kernel.dmesg_restrict` 0 or 1 (`proc_dointvec_minmax`),
/// `kernel.printk_devkmsg` `on`, `off` or `ratelimit` (`devkmsg_sysctl_set_loglvl`).
fn write_printk(printk: &crate::printk::Printk, file: text::Global, data: &[u8]) -> FsResult<()> {
    let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
    match file {
        text::Global::Printk => {
            let levels: Vec<i32> = text
                .split_whitespace()
                .take(4)
                .map(str::parse)
                .collect::<Result<_, _>>()
                .map_err(|_| Errno::EINVAL)?;
            printk.set_levels(&levels);
        }
        text::Global::DmesgRestrict => match sysctl_int(data)? {
            restrict @ (0 | 1) => printk.set_dmesg_restrict(restrict == 1),
            _ => return Err(Errno::EINVAL),
        },
        _ => {
            let mode = match text.trim_end_matches(['\n', '\0']) {
                "on" => crate::printk::Devkmsg::On,
                "off" => crate::printk::Devkmsg::Off,
                "ratelimit" => crate::printk::Devkmsg::Ratelimit,
                _ => return Err(Errno::EINVAL),
            };
            printk.set_devkmsg(mode);
        }
    }
    Ok(())
}

#[async_trait(?Send)]
impl Handle for FileHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let current = self
            .seccomp
            .as_ref()
            .map(|log| format!("{}\n", log.names()).into_bytes());
        let data = current.as_ref().unwrap_or(&self.data);
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    /// `comm_write`: the new name, cut to fit; a user namespace's map or `setgroups`; or a
    /// sysctl's new value.
    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        match self.write.as_ref().ok_or(Errno::EINVAL)? {
            Write::Acct(acct) => {
                let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
                let values = text
                    .split_whitespace()
                    .take(3)
                    .map(|word| word.parse::<i32>().map_err(|_| Errno::EINVAL))
                    .collect::<Result<Vec<_>, _>>()?;
                if values.is_empty() {
                    return Err(Errno::EINVAL);
                }
                acct.parameters.lock_or_recover()[..values.len()].copy_from_slice(&values);
            }

            Write::Seccomp(source, opener) => {
                // Linux seccomp.c:write_actions_logged checks current credentials
                // at write time, even for an fd opened before a capability drop.
                let current = wasixv2_syscalls::current_task().map_or(*opener, |task| task.0);
                if current != 0
                    && !source
                        .cred_of(current)
                        .is_some_and(|cred| cred.capable_init(Cap::SysAdmin))
                {
                    return Err(Errno::EPERM);
                }
                source.seccomp_logging.set_names(data, offset)?;
            }
            Write::Map(ns, kind, cred) => return ns.write_map(*kind, cred, offset, data),
            Write::Setgroups(ns) => return ns.write_setgroups(offset, data),
            // `timens_offsets_write` and `proc_timens_set_offset`.
            Write::TimensOffsets((source, scope), cred) => {
                if offset != 0 || data.len() >= crate::mm::PAGE_SIZE as usize {
                    return Err(Errno::EINVAL);
                }
                let (offsets, taken) = parse_timens_offsets(data)?;
                let task = source.task(scope.pid, scope.tid).ok_or(Errno::ESRCH)?;
                let ns = task.ns.as_ref().ok_or(Errno::ESRCH)?;
                let ns = &ns.time_for_children;
                if !cred.ns_capable(ns.user_ns(), Cap::SysTime) {
                    return Err(Errno::EPERM);
                }
                ns.set_offsets(&offsets, source.host.monotonic())?;
                return Ok(taken);
            }
            Write::Ucount(ns, kind) => ns.set_ucount_max(*kind, sysctl_int(data)?)?,
            Write::Protected(vfs, which) => {
                let vfs = vfs.upgrade().ok_or(Errno::ENODEV)?;
                vfs.set_protected(*which, i64::from(sysctl_i32(data)?))?;
            }
            Write::Pressure => return Err(Errno::EOPNOTSUPP),
            Write::Comm(source, scope) => {
                source
                    .set_comm(scope.pid, scope.tid, data)
                    .ok_or(Errno::ESRCH)?;
            }
            Write::PidMax(ns) => ns.set_pid_max(sysctl_int(data)?)?,
            // `pid_ns_ctl_handler`.
            Write::NsLastPid(_, false) => return Err(Errno::EPERM),
            Write::NsLastPid(ns, true) => ns.set_last_pid(sysctl_int(data)?)?,
            Write::NrOpen(nr_open) => nr_open.set(sysctl_int(data)?)?,
            Write::PipeMaxSize(max) => max.set(sysctl_int(data)?)?,
            Write::AioMaxNr(aio) => aio.set_max(sysctl_int(data)?),
            Write::Vm(vm, file) => match file {
                text::Global::Overcommit => vm.set_overcommit_memory(sysctl_i32(data)?)?,
                text::Global::OvercommitRatio => vm.set_overcommit_ratio(sysctl_i32(data)?),
                text::Global::OvercommitKbytes => vm.set_overcommit_kbytes(sysctl_int(data)?),
                _ => vm.set_max_map_count(sysctl_i32(data)?)?,
            },
            Write::OomAdj(process, legacy, capable) => {
                let value = sysctl_i32(data)?;
                set_oom_adj(process, value, *legacy, *capable)?;
            }
            Write::Loginuid(process, cred, sessions) => {
                if offset != 0 {
                    return Err(Errno::EINVAL);
                }
                set_loginuid(process, cred, sessions, sysctl_int(data)?)?;
            }
            Write::Timerslack(source, scope) => {
                let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
                let slack: u64 = text.trim().parse().map_err(|_| Errno::EINVAL)?;
                source.set_timer_slack(*scope, slack).ok_or(Errno::ESRCH)?;
            }
            Write::Discard => {}
            Write::Core(core, file) => match file {
                text::Global::CorePattern => core.set_pattern(data)?,
                text::Global::CoreUsesPid => core.set_uses_pid(sysctl_int(data)? != 0),
                _ => core.set_pipe_limit(sysctl_int(data)?),
            },
            // `proc_coredump_filter_write`: the bits a filter has, from any base.
            Write::CoredumpFilter(process) => {
                let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?.trim();
                let value = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None if text.len() > 1 && text.starts_with('0') => {
                        u32::from_str_radix(&text[1..], 8)
                    }
                    None => text.parse(),
                }
                .map_err(|_| Errno::EINVAL)?;
                let mask = crate::coredump::COREDUMP_FILTER_MASK;
                process.ident.lock_or_recover().coredump_filter = value & mask;
            }
            Write::Tunable(tunables, tunable) => {
                let value: i64 = std::str::from_utf8(data)
                    .map_err(|_| Errno::EINVAL)?
                    .trim()
                    .parse()
                    .map_err(|_| Errno::EINVAL)?;
                tunables.set(*tunable, value)?;
            }
            // `write_sysrq_trigger`: every key written, whatever `kernel.sysrq` allows.
            Write::Sysrq(source) => {
                for &key in data {
                    source.sysrq(key).await;
                }
            }
            // `drop_caches_sysctl_handler`: there is no page cache to drop.
            Write::DropCaches(source, opener) => match sysctl_int(data)? {
                value @ 1..=4 => source.drop_caches(*opener, value),
                _ => return Err(Errno::EINVAL),
            },
            Write::Printk(_, _, false) => return Err(Errno::EPERM),
            Write::Printk(printk, file, true) => write_printk(printk, *file, data)?,
            Write::Taint(_, false) => return Err(Errno::EPERM),
            Write::Taint(taint, true) => taint.add(sysctl_int(data)?),
            Write::Yama(_, false) => return Err(Errno::EPERM),
            Write::Yama(yama, true) => yama.set_scope(sysctl_i32(data)?)?,
            Write::Fs(sysctls, file) => {
                let value = sysctl_i32(data)?;
                match file {
                    text::Global::DirNotifyEnable => sysctls.set_dir_notify_enable(value),
                    text::Global::LeasesEnable => sysctls.set_leases_enable(value),
                    text::Global::LeaseBreakTime => sysctls.set_lease_break_time(value),
                    _ => return Err(Errno::EINVAL),
                }
            }
        }
        Ok(data.len())
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        Ok((0, self.write_at(data, 0).await?))
    }
}
