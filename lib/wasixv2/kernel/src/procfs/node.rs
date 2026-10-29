//! The nodes of the process filesystem: what each name is, its inode number, metadata,
//! directory listing and link target.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::{Errno, Fd};
use wasixv2_vfs::{
    Attr, Cap, Ctx, DirEntry, DirSink, FileType, FsResult, Handle, Link, Node, OpenOptions, SetAttr,
};

use super::{Source, text};
use crate::{
    cred::Cred,
    devinet,
    file::NrOpen,
    ns::{NetNs, NsKind, NsLink, PidNs, UserNs},
    sync::MutexExt,
    userns::MapKind,
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
    /// `/proc/sys/net/ipv4/conf/<scope>`.
    Devconf(devinet::Scope),
    /// A value there, one of [`devinet::VARS`] by index.
    DevconfVar(devinet::Scope, usize),
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
    /// `/proc/sys/net/ipv4/conf`, whose directories are `all`, `default` and the interfaces.
    Devconf,
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
    // 28
    (11, "net", Static::Dir),
    (28, "ipv4", Static::Dir),
    (29, "conf", Static::Devconf),
    // 31
    (11, "user", Static::Dir),
    (31, "max_user_namespaces", ucount(NsKind::User)),
    (31, "max_pid_namespaces", ucount(NsKind::Pid)),
    (31, "max_uts_namespaces", ucount(NsKind::Uts)),
    (31, "max_ipc_namespaces", ucount(NsKind::Ipc)),
    (31, "max_net_namespaces", ucount(NsKind::Net)),
    (31, "max_mnt_namespaces", ucount(NsKind::Mnt)),
    (31, "max_cgroup_namespaces", ucount(NsKind::Cgroup)),
    (31, "max_time_namespaces", ucount(NsKind::Time)),
    (13, "overflowuid", Static::File(text::Global::OverflowUid)),
    (13, "overflowgid", Static::File(text::Global::OverflowGid)),
    (13, "cap_last_cap", Static::File(text::Global::CapLastCap)),
];

/// `/proc/sys/user/max_*_namespaces` for `kind`.
const fn ucount(kind: NsKind) -> Static {
    Static::File(text::Global::MaxNamespaces(kind))
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
    File(text::PerPid),
}

/// The entries of `/proc/<pid>` and `/proc/<pid>/task/<tid>`: name, kind, mode. `task` only
/// exists for the process.
const PID_ENTRIES: &[(&str, PidEntry, u32)] = &[
    ("fd", PidEntry::Fds, 0o500),
    ("fdinfo", PidEntry::FdInfos, 0o555),
    ("task", PidEntry::Tasks, 0o555),
    ("exe", PidEntry::Exe, 0o777),
    ("cwd", PidEntry::Cwd, 0o777),
    ("root", PidEntry::Root, 0o777),
    ("ns", PidEntry::Namespaces, 0o511),
    ("net", PidEntry::Net, 0o555),
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
];

/// The files of `/proc/<pid>/net`, which show the task's network namespace.
const NET_ENTRIES: &[(&str, text::Net)] = &[
    ("tcp", text::Net::Tcp(false)),
    ("tcp6", text::Net::Tcp(true)),
    ("udp", text::Net::Udp(false)),
    ("udp6", text::Net::Udp(true)),
    ("unix", text::Net::Unix),
    ("dev", text::Net::Dev),
];

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
                if let Entry::Static(index) = self.entry
                    && matches!(STATIC[index].2, Static::Devconf)
                {
                    out.extend(self.devconf_dirs());
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
                    if *kind == PidEntry::Tasks && scope.tid.is_some() {
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
                    for (index, (name, _)) in NET_ENTRIES.iter().enumerate() {
                        out.push((
                            index as u64,
                            name.as_bytes().to_vec(),
                            Entry::Net(scope, index),
                        ));
                    }
                }
                _ => return Err(Errno::ENOTDIR),
            },
            Entry::Devconf(scope) => {
                for (index, var) in devinet::VARS.iter().enumerate() {
                    let name = var.name.as_bytes().to_vec();
                    out.push((index as u64, name, Entry::DevconfVar(scope, index)));
                }
            }
            Entry::Fd(..)
            | Entry::FdInfo(..)
            | Entry::Ns(..)
            | Entry::Net(..)
            | Entry::DevconfVar(..) => {
                return Err(Errno::ENOTDIR);
            }
        }
        Ok(out)
    }

    /// The directories of `/proc/sys/net/ipv4/conf`: `all`, `default` and one per interface.
    /// Nodes are looked up without the caller, so the interfaces are those of the initial
    /// network namespace; every namespace has the loopback interface, and each file shows
    /// the opener's namespace ([`ProcNode::open`]).
    fn devconf_dirs(&self) -> Vec<(u64, Vec<u8>, Entry)> {
        let mut out = vec![
            (0, b"all".to_vec(), Entry::Devconf(devinet::Scope::All)),
            (
                1,
                b"default".to_vec(),
                Entry::Devconf(devinet::Scope::Default),
            ),
        ];
        let network = self.source.ns.net.network.as_deref();
        for dev in devinet::devices(network) {
            let entry = Entry::Devconf(devinet::Scope::Dev(dev.index));
            out.push((2 + u64::from(dev.index), dev.name.into_bytes(), entry));
        }
        out
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
                Static::Dir | Static::Devconf => 0o555,
                Static::File(file) => file.mode(),
                Static::Link(_) | Static::SelfLink | Static::ThreadSelf => 0o777,
            },
            Entry::Pid(_, index) => PID_ENTRIES[index].2,
            Entry::FdInfo(..) | Entry::Net(..) => 0o444,
            Entry::Devconf(_) => 0o555,
            Entry::DevconfVar(_, var) => match devinet::VARS[var].writable {
                true => 0o644,
                false => 0o444,
            },
            Entry::Ns(..) => 0o777,
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
            Entry::Root | Entry::Static(_) | Entry::Devconf(_) | Entry::DevconfVar(..) => {
                return (0, 0);
            }
            Entry::Dir(scope)
            | Entry::Pid(scope, _)
            | Entry::Fd(scope, _)
            | Entry::FdInfo(scope, _)
            | Entry::Ns(scope, _)
            | Entry::Net(scope, _) => scope,
        };
        let Some(info) = self.source.process(scope.pid) else {
            return (0, 0);
        };
        let process = info.process;
        let cred = process.cred();
        if !process.ident.lock_or_recover().dumpable {
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
            let dumpable = target.process.ident.lock_or_recover().dumpable;
            let cred = caller.process.cred();
            if !cred.may_read_task(&target.process.cred(), dumpable, true) {
                return Err(Errno::EACCES);
            }
        }
        let ns = link.get(
            &task.ns,
            &target.process.pid_ns,
            &target.process.cred().user_ns,
        );
        Ok(Link::Jump(self.source.nsfs.path(&ns)))
    }
}

/// Inode numbers. Static nodes count from 1; a process or thread directory and everything in
/// it carry the pid or tid above bit 30, with the entry below.
fn ino(entry: Entry) -> u64 {
    // `/proc/sys/net/ipv4/conf`'s nodes carry their scope above bit 8 under bit 62.
    let devconf = |scope: devinet::Scope, sub: u64| {
        let key = match scope {
            devinet::Scope::All => 0,
            devinet::Scope::Default => 1,
            devinet::Scope::Dev(index) => 2 + u64::from(index),
        };
        (1 << 62) | (key << 8) | sub
    };
    let scoped = |scope: Scope, sub: u64| {
        let key = match scope.tid {
            None => u64::from(scope.pid),
            Some(tid) => (1 << 31) | u64::from(tid),
        };
        (1 << 63) | (key << 30) | sub
    };
    match entry {
        Entry::Root => 1,
        Entry::Static(index) => 2 + index as u64,
        Entry::Dir(scope) => scoped(scope, 0),
        Entry::Pid(scope, index) => scoped(scope, 1 + index as u64),
        Entry::Fd(scope, fd) => scoped(scope, (1 << 29) | u64::from(fd)),
        Entry::FdInfo(scope, fd) => scoped(scope, (1 << 28) | u64::from(fd)),
        Entry::Ns(scope, index) => scoped(scope, (1 << 27) | index as u64),
        Entry::Net(scope, index) => scoped(scope, (1 << 26) | index as u64),
        Entry::Devconf(scope) => devconf(scope, 0),
        Entry::DevconfVar(scope, var) => devconf(scope, 1 + var as u64),
    }
}

fn file_type(entry: Entry) -> FileType {
    match entry {
        Entry::Root | Entry::Dir(_) => FileType::Directory,
        Entry::Static(index) => match STATIC[index].2 {
            Static::Dir | Static::Devconf => FileType::Directory,
            Static::File(_) => FileType::Regular,
            Static::Link(_) | Static::SelfLink | Static::ThreadSelf => FileType::Symlink,
        },
        Entry::Pid(_, index) => match PID_ENTRIES[index].1 {
            PidEntry::Fds
            | PidEntry::FdInfos
            | PidEntry::Tasks
            | PidEntry::Namespaces
            | PidEntry::Net => FileType::Directory,
            PidEntry::Exe | PidEntry::Cwd | PidEntry::Root => FileType::Symlink,
            PidEntry::File(_) => FileType::Regular,
        },
        Entry::Fd(..) | Entry::Ns(..) => FileType::Symlink,
        Entry::FdInfo(..) | Entry::Net(..) | Entry::DevconfVar(..) => FileType::Regular,
        Entry::Devconf(_) => FileType::Directory,
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
        let (data, comm) = match self.entry {
            Entry::Static(index) => match STATIC[index].2 {
                Static::File(file) => {
                    let ns = self.source.ns_of(options.task);
                    let pid_ns = self.source.pid_ns_of(options.task);
                    let opener = self.source.cred_of(options.task);
                    let user_ns = self.source.user_ns_of(options.task);
                    let data = text::global(&self.source, &ns, &pid_ns, &user_ns, file)?;
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
                        text::Global::MaxNamespaces(kind) => {
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
                        _ => None,
                    };
                    (data, write)
                }
                _ => return Err(Errno::ENXIO),
            },
            Entry::Pid(scope, index) => match PID_ENTRIES[index].1 {
                PidEntry::File(file) => {
                    let reader = self.source.user_ns_of(options.task);
                    let data = text::per_pid(&self.source, scope, file, &reader)?;
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
                (text::net(&task.ns.net, NET_ENTRIES[index].1), None)
            }
            // The opener's network namespace, as `net_ctl_header_lookup` takes `current`'s.
            Entry::DevconfVar(scope, var) => {
                // `proc_sys_permission`: root does not override a read-only sysctl's mode.
                if options.write && !devinet::VARS[var].writable {
                    return Err(Errno::EACCES);
                }
                let net = self.source.ns_of(options.task).net.clone();
                let value = {
                    let devs = devinet::devices(net.network.as_deref());
                    let dev = match scope {
                        devinet::Scope::Dev(index) => {
                            Some(devs.into_iter().find(|dev| dev.index == index))
                        }
                        _ => None,
                    };
                    // An interface the opener's namespace does not have.
                    if dev.as_ref().is_some_and(Option::is_none) {
                        return Err(Errno::ENOENT);
                    }
                    let devconf = net.devconf.lock_or_recover();
                    devconf.get(scope, var, dev.flatten().as_ref())
                };
                let write = options.write.then_some(Write::Devconf(net, scope, var));
                (format!("{value}\n").into_bytes(), write)
            }
            _ => return Err(Errno::ENXIO),
        };
        Ok(Box::new(FileHandle { data, write: comm }))
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
            _ => return Ok(None),
        };
        let target = self.user_ns(scope)?;
        let cred = self.source.cred_of(opener).ok_or(Errno::EPERM)?;
        Ok(Some(Write::Map(target, map, Box::new(cred))))
    }

    /// The user namespace of the process whose directory `scope` is.
    fn user_ns(&self, scope: Scope) -> FsResult<Arc<UserNs>> {
        let info = self.source.process(scope.pid).ok_or(Errno::ESRCH)?;
        Ok(info.process.cred().user_ns)
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

/// An open file: its contents as they were at open, and what a write to it changes.
struct FileHandle {
    data: Vec<u8>,
    write: Option<Write>,
}

/// What a writable file sets.
enum Write {
    /// A task's name.
    Comm(Arc<Source>, Scope),
    /// `kernel.pid_max` of the opener's pid namespace.
    PidMax(Arc<PidNs>),
    /// `kernel.ns_last_pid` of the opener's pid namespace, if it may
    /// (`checkpoint_restore_ns_capable`).
    NsLastPid(Arc<PidNs>, bool),
    /// `fs.nr_open`.
    NrOpen(Arc<NrOpen>),
    /// A value of `/proc/sys/net/ipv4/conf` in the opener's network namespace.
    Devconf(Arc<NetNs>, devinet::Scope, usize),
    /// A map of a user namespace, written with the opener's credentials.
    Map(Arc<UserNs>, MapKind, Box<Cred>),
    /// Whether a user namespace allows `setgroups`.
    Setgroups(Arc<UserNs>),
    /// `user.max_*_namespaces` of the opener's user namespace.
    Ucount(Arc<UserNs>, NsKind),
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

#[async_trait(?Send)]
impl Handle for FileHandle {
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.data.len());
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }

    /// `comm_write`: the new name, cut to fit; a user namespace's map or `setgroups`; or a
    /// sysctl's new value.
    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        match self.write.as_ref().ok_or(Errno::EINVAL)? {
            Write::Map(ns, kind, cred) => return ns.write_map(*kind, cred, offset, data),
            Write::Setgroups(ns) => return ns.write_setgroups(offset, data),
            Write::Ucount(ns, kind) => ns.set_ucount_max(*kind, sysctl_int(data)?)?,
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
            Write::Devconf(net, scope, var) => {
                let value = sysctl_i32(data)?;
                net.devconf.lock_or_recover().set(*scope, *var, value)?;
            }
        }
        Ok(data.len())
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        Ok((0, self.write_at(data, 0).await?))
    }
}
