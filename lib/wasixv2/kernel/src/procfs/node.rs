//! The nodes of the process filesystem: what each name is, its inode number, metadata,
//! directory listing and link target.

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use wasixv2_abi::OpenFlags;
use wasixv2_syscalls::{Errno, Fd};
use wasixv2_vfs::{
    Attr, Ctx, DirEntry, DirSink, FileType, FsResult, Handle, Link, Node, OpenOptions, SetAttr,
};

use super::{Source, text};
use crate::{ns::NsLink, sync::MutexExt};

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
    (ROOT, "net", Static::Dir),
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
    (
        14,
        "overcommit_memory",
        Static::File(text::Global::Overcommit),
    ),
    (14, "mmap_min_addr", Static::File(text::Global::MmapMinAddr)),
    (15, "file-max", Static::File(text::Global::FileMax)),
    (15, "nr_open", Static::File(text::Global::NrOpen)),
    (15, "pipe-max-size", Static::File(text::Global::PipeMaxSize)),
    (12, "tcp", Static::File(text::Global::Tcp(false))),
    (12, "tcp6", Static::File(text::Global::Tcp(true))),
    (12, "udp", Static::File(text::Global::Udp(false))),
    (12, "udp6", Static::File(text::Global::Udp(true))),
    (12, "unix", Static::File(text::Global::Unix)),
    (12, "dev", Static::File(text::Global::NetDev)),
];

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
    /// `ns/`.
    Namespaces,
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
                    for pid in self.source.pids() {
                        let scope = Scope { pid, tid: None };
                        let name = pid.to_string().into_bytes();
                        out.push((FIRST_PID_POS + u64::from(pid), name, Entry::Dir(scope)));
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
                    for tid in self.source.threads(scope.pid) {
                        let thread = Scope {
                            pid: scope.pid,
                            tid: Some(tid),
                        };
                        let name = tid.to_string().into_bytes();
                        out.push((u64::from(tid), name, Entry::Dir(thread)));
                    }
                }
                kind @ (PidEntry::Fds | PidEntry::FdInfos) => {
                    let task = self
                        .source
                        .task(scope.pid, scope.tid)
                        .ok_or(Errno::ENOENT)?;
                    let fds: Vec<u32> = task
                        .files
                        .lock()
                        .unwrap()
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
                _ => return Err(Errno::ENOTDIR),
            },
            Entry::Fd(..) | Entry::FdInfo(..) | Entry::Ns(..) => return Err(Errno::ENOTDIR),
        }
        Ok(out)
    }

    /// The open file behind `fd/<n>` or `fdinfo/<n>`, with its close-on-exec flag.
    fn open_file(&self, scope: Scope, fd: u32) -> FsResult<(Arc<crate::file::OpenFile>, bool)> {
        let task = self
            .source
            .task(scope.pid, scope.tid)
            .ok_or(Errno::ENOENT)?;
        let files = task.files.lock().unwrap();
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
                Static::Link(_) | Static::SelfLink | Static::ThreadSelf => 0o777,
            },
            Entry::Pid(_, index) => PID_ENTRIES[index].2,
            Entry::FdInfo(..) => 0o444,
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
            Entry::Root | Entry::Static(_) => return (0, 0),
            Entry::Dir(scope)
            | Entry::Pid(scope, _)
            | Entry::Fd(scope, _)
            | Entry::FdInfo(scope, _)
            | Entry::Ns(scope, _) => scope,
        };
        let Some(info) = self.source.process(scope.pid) else {
            return (0, 0);
        };
        let process = info.process;
        if !process.ident.lock().unwrap().dumpable {
            return (0, 0);
        }
        let cred = process.cred();
        (cred.user.effective, cred.group.effective)
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
        let ns = link.get(&task.ns, &target.process.pid_ns);
        Ok(Link::Jump(self.source.nsfs.path(&ns)))
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
        Entry::Static(index) => 2 + index as u64,
        Entry::Dir(scope) => scoped(scope, 0),
        Entry::Pid(scope, index) => scoped(scope, 1 + index as u64),
        Entry::Fd(scope, fd) => scoped(scope, (1 << 29) | u64::from(fd)),
        Entry::FdInfo(scope, fd) => scoped(scope, (1 << 28) | u64::from(fd)),
        Entry::Ns(scope, index) => scoped(scope, (1 << 27) | index as u64),
    }
}

fn file_type(entry: Entry) -> FileType {
    match entry {
        Entry::Root | Entry::Dir(_) => FileType::Directory,
        Entry::Static(index) => match STATIC[index].2 {
            Static::Dir => FileType::Directory,
            Static::File(_) => FileType::Regular,
            Static::Link(_) | Static::SelfLink | Static::ThreadSelf => FileType::Symlink,
        },
        Entry::Pid(_, index) => match PID_ENTRIES[index].1 {
            PidEntry::Fds | PidEntry::FdInfos | PidEntry::Tasks | PidEntry::Namespaces => {
                FileType::Directory
            }
            PidEntry::Exe | PidEntry::Cwd | PidEntry::Root => FileType::Symlink,
            PidEntry::File(_) => FileType::Regular,
        },
        Entry::Fd(..) | Entry::Ns(..) => FileType::Symlink,
        Entry::FdInfo(..) => FileType::Regular,
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

    /// `proc_setattr`: modes and owners are the kernel's.
    async fn setattr(&self, _: &SetAttr) -> FsResult<Attr> {
        Err(Errno::EPERM)
    }

    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        if self.file_type() != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        // Process and descriptor directories are looked up by number, not by listing them.
        let number = std::str::from_utf8(name)
            .ok()
            .filter(|s| !s.starts_with('0') || *s == "0")
            .and_then(|s| s.parse::<u32>().ok());
        match (self.entry, number) {
            (Entry::Root, Some(pid)) => {
                let scope = Scope { pid, tid: None };
                self.alive(scope)?;
                return Ok(self.node(Entry::Dir(scope)));
            }
            (Entry::Pid(scope, index), Some(n)) => {
                let entry = match PID_ENTRIES[index].1 {
                    PidEntry::Tasks => {
                        let thread = Scope {
                            pid: scope.pid,
                            tid: Some(n),
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
                Static::SelfLink => {
                    let tgid = self.source.tgid_of(cx.task).ok_or(Errno::ENOENT)?;
                    return Ok(Link::Body(tgid.to_string().into_bytes()));
                }
                Static::ThreadSelf => {
                    let tgid = self.source.tgid_of(cx.task).ok_or(Errno::ENOENT)?;
                    let body = format!("{tgid}/task/{}", cx.task);
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
                process.process.exe.lock().unwrap().clone()
            }
            PidEntry::Cwd | PidEntry::Root => {
                let task = self.source.task(scope.pid, scope.tid);
                task.map(|task| {
                    let fs = task.fs.lock().unwrap();
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
                Static::File(file) => (text::global(&self.source, file)?, None),
                _ => return Err(Errno::ENXIO),
            },
            Entry::Pid(scope, index) => match PID_ENTRIES[index].1 {
                PidEntry::File(file) => {
                    let comm = (file == text::PerPid::Comm && options.write).then_some(scope);
                    (text::per_pid(&self.source, scope, file)?, comm)
                }
                _ => return Err(Errno::ENXIO),
            },
            Entry::FdInfo(scope, fd) => {
                let (file, cloexec) = self.open_file(scope, fd)?;
                (text::fdinfo(&file, cloexec), None)
            }
            _ => return Err(Errno::ENXIO),
        };
        Ok(Box::new(FileHandle {
            data,
            comm: comm.map(|scope| (self.source.clone(), scope)),
        }))
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

/// An open file: its contents as they were at open. `comm` is writable.
struct FileHandle {
    data: Vec<u8>,
    comm: Option<(Arc<Source>, Scope)>,
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

    /// `comm_write`: the new name, cut to fit.
    async fn write_at(&self, data: &[u8], _: u64) -> FsResult<usize> {
        let (source, scope) = self.comm.as_ref().ok_or(Errno::EINVAL)?;
        source
            .set_comm(scope.pid, scope.tid, data)
            .ok_or(Errno::ESRCH)?;
        Ok(data.len())
    }

    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        Ok((0, self.write_at(data, 0).await?))
    }
}
