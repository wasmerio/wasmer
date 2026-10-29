//! The process filesystem (`fs/proc`): the kernel's live state as files, following
//! `Documentation/filesystems/proc.rst`.
//!
//! Every node is computed from the kernel when it is looked up or opened, never stored: a
//! directory per process and per thread, their descriptors as magic links that jump to the
//! open file's object (`proc_fd_link`), and the system-wide files. A file's contents are
//! produced when it is opened and read from that snapshot, as `seq_file` does for the first
//! read.

mod introspect;
mod node;
mod sysnet;
mod text;

pub(crate) use sysnet::NetSysctl;
pub(crate) use text::root_in_fs;

use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use wasixv2_host::Host;
use wasixv2_syscalls::{CpuTimes, TaskId};
use wasixv2_vfs::{
    Clock, Filesystem, FsInfo, FsResult, NAME_MAX, Node, Path, StatFs, Timestamp, Vfs,
};

use crate::{
    Kernel, Process, TaskTable,
    file::{FdTable, NrOpen},
    fs::{FsContext, HostClock},
    ident::Comm,
    mm::{Mm, VmSysctls},
    ns::{NsProxy, PidNs, UserNs},
    nsfs::Nsfs,
    pid::{Pids, Upid},
    pipe::PipeMaxSize,
    process::Tree,
    sched::Sched,
    signal::TaskSignals,
    sync::MutexExt,
    taint::Taint,
};

/// `PROC_SUPER_MAGIC`.
pub(crate) const PROC_SUPER_MAGIC: u32 = 0x9fa0;

impl Kernel {
    /// A process filesystem for this kernel, to mount at `/proc`, showing the initial pid
    /// namespace.
    pub fn procfs(&self) -> Arc<dyn Filesystem> {
        self.procfs_in(self.init_ns.pid_for_children.clone())
    }

    /// A process filesystem showing the processes of `pid_ns` and below, by their pids there
    /// (`proc_fill_super` with the mounter's `pid_ns`).
    pub(crate) fn procfs_in(&self, pid_ns: Arc<PidNs>) -> Arc<dyn Filesystem> {
        let mounted = HostClock(self.host.clone()).now();
        let source = Source {
            sysnet_names: Mutex::default(),
            state: Arc::downgrade(&self.state),
            tree: Arc::downgrade(&self.procs.tree),
            vfs: Arc::downgrade(&self.vfs),
            host: self.host.clone(),
            pid_ns,
            pids: self.pids.clone(),
            ns: self.init_ns.clone(),
            nsfs: self.pseudo.nsfs.clone(),
            filesystems: self.filesystems.clone(),
            sysctls: self.sysctls.clone(),
            mounted,
            nr_open: self.nr_open.clone(),
            taint: self.taint.clone(),
            yama: self.yama.clone(),
            audit_sessions: self.audit_sessions.clone(),
            pipe_max_size: self.pipe_max_size.clone(),
            aio: self.aio.clone(),
            vm: self.vm.clone(),
            keys: self.keys.clone(),
            cgroups: self.cgroups.clone(),
            printk: self.printk.clone(),
            acct: self.acct.clone(),
            seccomp_logging: self.seccomp_logging.clone(),
            coredump: self.coredump.clone(),
            tunables: self.tunables.clone(),
            boot_id: self.boot_id,
        };
        Arc::new(ProcFs {
            source: Arc::new(source),
        })
    }
}

/// `generate_random_uuid`'s marks on 16 random bytes: version 4, the RFC 4122 variant.
pub(crate) fn uuid_v4(mut id: [u8; 16]) -> [u8; 16] {
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// Whether `path` is `/proc/kmsg`, which opens as the kernel log (`proc_kmsg_operations`).
pub(crate) fn is_kmsg(path: &Path) -> bool {
    if path.dentry.sb().info.magic != PROC_SUPER_MAGIC {
        return false;
    }
    let node = path.dentry.node();
    let node: Option<&node::ProcNode> = node.as_any().downcast_ref();
    node.is_some_and(|node| node.is_kmsg())
}

/// The process whose `/proc/<pid>` directory `path` is, if it is one (`tgid_pidfd_to_pid`):
/// thread directories (`/proc/<pid>/task/<tid>`) are not.
pub(crate) fn tgid_dir(path: &Path) -> Option<u32> {
    if path.dentry.sb().info.magic != PROC_SUPER_MAGIC {
        return None;
    }
    let node = path.dentry.node();
    let node: &node::ProcNode = node.as_any().downcast_ref()?;
    node.tgid_dir()
}

struct ProcFs {
    source: Arc<Source>,
}

#[async_trait(?Send)]
impl Filesystem for ProcFs {
    fn info(&self) -> FsInfo {
        FsInfo {
            name: "proc",
            magic: PROC_SUPER_MAGIC,
            name_max: NAME_MAX,
            read_only: false,
            dev: None,
            has_btime: false,
            // Process and fd entries come and go with the kernel's state (`pid_revalidate`).
            stable: false,
        }
    }

    fn root(&self) -> Arc<dyn Node> {
        node::ProcNode::root(self.source.clone())
    }

    async fn statfs(&self) -> FsResult<StatFs> {
        Ok(StatFs {
            block_size: 4096,
            name_max: NAME_MAX as u32,
            fragment_size: 4096,
            ..StatFs::default()
        })
    }
}

/// What the process filesystem reads: the kernel's state, held weakly so that a mounted
/// `/proc` does not keep the kernel alive.
struct Source {
    state: Weak<TaskTable>,
    tree: Weak<Mutex<Tree>>,
    vfs: Weak<Vfs>,
    host: Arc<dyn Host>,
    /// The pid namespace whose processes the instance shows, by their pids there.
    pid_ns: Arc<PidNs>,
    pids: Arc<Pids>,
    /// The initial namespaces, which the system files show the embedder.
    ns: Arc<NsProxy>,
    /// Where `/proc/<pid>/ns` links lead.
    nsfs: Nsfs,
    /// The filesystem types `/proc/filesystems` lists.
    filesystems: Arc<crate::fs::mount::Registry>,
    /// What `sys/fs` shows and sets.
    sysctls: Arc<crate::fs::FsSysctls>,
    /// Every node's timestamps: when the filesystem was made.
    mounted: Timestamp,
    /// `fs.nr_open`.
    nr_open: Arc<NrOpen>,
    /// The interface names `/proc/sys/net` has met, which number its directories.
    sysnet_names: Mutex<sysnet::Names>,
    /// `kernel.tainted`.
    taint: Arc<Taint>,
    /// `kernel.yama.ptrace_scope`.
    yama: Arc<crate::yama::Yama>,
    /// The audit session ids `loginuid` writes hand out.
    audit_sessions: Arc<std::sync::atomic::AtomicU32>,
    /// `fs.pipe-max-size`.
    pipe_max_size: Arc<PipeMaxSize>,
    /// `fs.aio-nr` and `fs.aio-max-nr`.
    aio: Arc<crate::aio::AioSysctl>,
    /// The `vm` tunables.
    vm: Arc<VmSysctls>,
    /// What `/proc/keys` and `/proc/key-users` show.
    keys: Arc<crate::keys::Keys>,
    /// The cgroup hierarchy, which `/proc/<pid>/cgroup` and `/proc/cgroups` show.
    cgroups: Arc<crate::cgroup::Hierarchy>,
    /// The kernel log's tunables.
    printk: Arc<crate::printk::Printk>,
    acct: Arc<crate::acct::Acct>,
    seccomp_logging: Arc<crate::seccomp::Logging>,
    /// The core dump tunables.
    coredump: Arc<crate::coredump::Coredump>,
    /// The integer sysctls.
    tunables: Arc<crate::sysctl::Tunables>,
    /// `kernel.random.boot_id`.
    boot_id: [u8; 16],
}

/// A process as the tree has it.
struct ProcessInfo {
    process: Arc<Process>,
    parent: u32,
    exit_signal: i32,
    zombie: bool,
    /// A zombie's cgroup.
    cgroup: Option<Arc<crate::cgroup::Cgroup>>,
}

/// A task's shared parts.
struct TaskInfo {
    cred: Arc<crate::cred::Cred>,
    id: TaskId,
    syscall: crate::entry::CallSlot,
    files: Arc<Mutex<FdTable>>,
    fs: Arc<Mutex<FsContext>>,
    signals: Arc<TaskSignals>,
    /// The address space (`get_task_mm`).
    mm: Arc<Mm>,
    comm: Comm,
    sched: Sched,
    /// `NoNewPrivs`, `Seccomp` and `Seccomp_filters`.
    seccomp: (bool, u32, u32),
    /// `None` once the task is done (`task->nsproxy` is `NULL`).
    ns: Option<Arc<NsProxy>>,
    cpu: CpuTimes,
    /// Its cgroup (`task_dfl_cgroup`).
    cgroup: Arc<crate::cgroup::Cgroup>,
}

impl Source {
    fn process(&self, pid: u32) -> Option<ProcessInfo> {
        let tree = self.tree.upgrade()?;
        let tree = tree.lock_or_recover();
        let node = tree.nodes.get(&pid)?;
        Some(ProcessInfo {
            process: node.process.clone(),
            parent: node.parent,
            exit_signal: node.exit_signal,
            zombie: node.is_zombie(),
            cgroup: node.css.as_ref().map(|css| css.cgroup()),
        })
    }

    /// `pid_nr_ns` in the instance's pid namespace: the pid shown for the id `id`, 0 for one
    /// outside it.
    fn nr(&self, id: u32) -> i32 {
        self.pids.nr(&self.pid_ns, id)
    }

    /// The id the instance shows as `nr` (`find_pid_ns`).
    fn find(&self, nr: u32) -> Option<u32> {
        self.pid_ns.find(i32::try_from(nr).ok()?)
    }

    /// The numbers of the id `id` in each pid namespace from the instance's down to its own.
    fn upids(&self, id: u32) -> Vec<Upid> {
        self.pids.upids_from(&self.pid_ns, id)
    }

    /// The id of every process of the machine, whatever its pid namespace.
    fn all_pids(&self) -> Vec<u32> {
        let Some(tree) = self.tree.upgrade() else {
            return Vec::new();
        };
        let tree = tree.lock_or_recover();
        tree.nodes.keys().copied().collect()
    }

    /// Every process in the instance's pid namespace, zombies included, by the id it has
    /// there, lowest first (`next_tgid`), with its id.
    fn pids(&self) -> Vec<(u32, u32)> {
        let Some(tree) = self.tree.upgrade() else {
            return Vec::new();
        };
        let tree = tree.lock_or_recover();
        self.pid_ns
            .members()
            .into_iter()
            .filter(|(_, id)| tree.nodes.contains_key(id))
            .collect()
    }

    /// The threads of process `pid`, by tid in the instance's pid namespace, lowest first,
    /// with their ids.
    fn threads(&self, pid: u32) -> Vec<(u32, u32)> {
        let Some(state) = self.state.upgrade() else {
            return Vec::new();
        };
        let state = state.read_or_recover();
        let mut tids: Vec<(u32, u32)> = state
            .tasks
            .iter()
            .filter(|(_, task)| task.process.pid.0 == pid)
            .map(|(tid, _)| (self.nr(tid.0) as u32, tid.0))
            .filter(|&(nr, _)| nr != 0)
            .collect();
        tids.sort_unstable();
        tids
    }

    /// The task `tid` of process `pid`, or for `None` the process's leader (its first task
    /// left, once the leader exited).
    fn task(&self, pid: u32, tid: Option<u32>) -> Option<TaskInfo> {
        let tid = match tid {
            Some(tid) => tid,
            None => self.threads(pid).first()?.1,
        };
        let state = self.state.upgrade()?;
        let state = state.read_or_recover();
        let task = state.tasks.get(&TaskId(tid))?;
        if task.process.pid.0 != pid {
            return None;
        }
        Some(TaskInfo {
            cred: task.cred.clone(),
            id: TaskId(tid),
            syscall: task.syscall.clone(),
            files: task.files.clone(),
            fs: task.fs.clone(),
            signals: task.signals.clone(),
            mm: task.mm.clone(),
            comm: task.comm,
            sched: task.sched.clone(),
            seccomp: task.seccomp.status(),
            ns: task.ns.clone(),
            cpu: task.cpu.times(),
            cgroup: task.css.cgroup(),
        })
    }

    /// The CPU time of every process of the machine, the reaped ones included.
    fn cpu_times(&self) -> CpuTimes {
        let Some(tree) = self.tree.upgrade() else {
            return CpuTimes::default();
        };
        let tree = tree.lock_or_recover();
        let live = tree.nodes.values().map(|node| node.process.cpu.times());
        live.fold(tree.reaped, CpuTimes::saturating_add)
    }

    /// Renames the task `tid` of process `pid`, or its leader (`comm_write`).
    fn set_comm(&self, pid: u32, tid: Option<u32>, name: &[u8]) -> Option<()> {
        let tid = match tid {
            Some(tid) => tid,
            None => self.threads(pid).first()?.1,
        };
        let state = self.state.upgrade()?;
        let mut state = state.lock_or_recover();
        let task = state.tasks.get_mut(&TaskId(tid))?;
        if task.process.pid.0 != pid {
            return None;
        }
        task.comm = Comm::new(name);
        Some(())
    }

    /// The address space of task `tid` of process `pid` (or its leader), as `get_task_mm` finds
    /// it; `None` once the process is a zombie.
    fn mm(&self, scope: node::Scope) -> Option<Arc<Mm>> {
        self.task(scope.pid, scope.tid).map(|task| task.mm)
    }

    /// The namespaces of task `tid`, which the system files show it: those of `current` for
    /// Linux's handlers. The embedder, and a task gone meanwhile, see the initial ones.
    fn ns_of(&self, tid: u32) -> Arc<NsProxy> {
        let ns = self.state.upgrade().and_then(|state| {
            let state = state.read_or_recover();
            state.tasks.get(&TaskId(tid))?.ns.clone()
        });
        ns.unwrap_or_else(|| self.ns.clone())
    }

    /// The credentials of the task `tid`, which opens a file (`f_cred`); none for the embedder
    /// or a task gone meanwhile.
    /// `/proc/keys` as the task `tid` sees it: the keys it may view, its keyrings making it
    /// possess some; nothing for the embedder, which has no keyrings.
    fn proc_keys(&self, tid: u32, user_ns: &UserNs) -> Vec<u8> {
        let Some(state) = self.state.upgrade() else {
            return Vec::new();
        };
        let state = state.read_or_recover();
        let Some(task) = state.tasks.get(&TaskId(tid)) else {
            return Vec::new();
        };
        let ctx = crate::keys::Ctx {
            cred: task.cred_override.as_ref().unwrap_or(&task.cred).clone(),
            now: self.host.realtime().as_secs(),
        };
        drop(state);
        self.keys.proc_keys(&ctx, user_ns)
    }

    fn cred_of(&self, tid: u32) -> Option<Arc<crate::cred::Cred>> {
        let state = self.state.upgrade()?;
        let state = state.read_or_recover();
        state
            .tasks
            .get(&TaskId(tid))
            .map(|t| t.cred.clone())
            .or_else(|| state.retired_creds.get(&TaskId(tid)).cloned())
    }

    /// `current_cred` for an opener; targets use `cred_of` (`__task_cred`).
    fn opener_cred(&self, tid: u32) -> Option<Arc<crate::cred::Cred>> {
        let state = self.state.upgrade()?;
        let state = state.read_or_recover();
        let task = state.tasks.get(&TaskId(tid))?;
        Some(task.cred_override.as_ref().unwrap_or(&task.cred).clone())
    }

    /// The user namespace of the task `tid`, which opens a file and sees its ids through it
    /// (`seq_user_ns`); without the task, the one that owns the instance's pid namespace.
    fn user_ns_of(&self, tid: u32) -> Arc<UserNs> {
        let ns = self.opener_cred(tid).map(|cred| cred.user_ns.clone());
        ns.unwrap_or_else(|| {
            self.pid_ns
                .owner_ns()
                .expect("a pid namespace has an owner")
                .clone()
        })
    }

    /// Whether the task `tid` may set where the pid namespace `ns`'s next pid search starts
    /// (`checkpoint_restore_ns_capable` over its owner).
    fn may_checkpoint_restore(&self, tid: u32, ns: &PidNs) -> bool {
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let state = state.read_or_recover();
        state.tasks.get(&TaskId(tid)).is_some_and(|task| {
            let cred = task.cred.clone();
            cred.ns_capable(ns.user_ns(), wasixv2_vfs::Cap::SysAdmin)
                || cred.ns_capable(ns.user_ns(), wasixv2_vfs::Cap::CheckpointRestore)
        })
    }

    /// The thread group of the task `tid`.
    fn tgid_of(&self, tid: u32) -> Option<u32> {
        let state = self.state.upgrade()?;
        let state = state.read_or_recover();
        Some(state.tasks.get(&TaskId(tid))?.process.pid.0)
    }

    /// `timerslack_ns_write`: the task's timer slack, its default for 0.
    fn set_timer_slack(&self, scope: node::Scope, slack: u64) -> Option<()> {
        let tid = match scope.tid {
            Some(tid) => tid,
            None => self.threads(scope.pid).first()?.1,
        };
        let state = self.state.upgrade()?;
        let mut state = state.lock_or_recover();
        state
            .tasks
            .get_mut(&TaskId(tid))?
            .sched
            .set_timer_slack(slack);
        Some(())
    }

    /// `__handle_sysrq` for `key`: the actions the machine has (`0`-`9` the console log level,
    /// `e` SIGTERM and `i` SIGKILL to every process but init, `m` the memory, `s` sync); any
    /// other key logs the help, which lists them.
    async fn sysrq(&self, key: u8) {
        use crate::printk::Level;
        let log = |text: &str| self.printk.log(Level::Info, text);
        match key {
            b'0'..=b'9' => {
                let level = i32::from(key - b'0');
                log("sysrq: Changing Loglevel");
                self.printk.set_levels(&[level]);
                log(&format!("sysrq: Loglevel set to {level}"));
            }
            b'e' | b'i' => {
                let (sig, what) = match key {
                    b'e' => (wasixv2_abi::Signal::SIGTERM, "Terminate All Tasks"),
                    _ => (wasixv2_abi::Signal::SIGKILL, "Kill All Tasks"),
                };
                log(&format!("sysrq: {what}"));
                self.signal_all_but_init(sig.0);
            }
            b'm' => {
                log("sysrq: Show Memory");
                let meminfo = String::from_utf8_lossy(&text::meminfo(self)).into_owned();
                for line in meminfo.lines().take(2) {
                    log(line);
                }
            }
            b's' => {
                log("sysrq: Emergency Sync");
                if let Some(vfs) = self.vfs.upgrade() {
                    for sb in vfs.superblocks() {
                        let _ = sb.fs.sync().await;
                    }
                }
                log("Emergency Sync complete");
            }
            _ => log(
                "sysrq: HELP : loglevel(0-9) terminate-all-tasks(e) kill-all-tasks(i) \
                 show-memory-usage(m) sync(s)",
            ),
        }
    }

    /// `send_sig_all`: `sig` to every process but the machine's init.
    fn signal_all_but_init(&self, sig: i32) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let groups: Vec<_> = {
            let state = state.read_or_recover();
            state
                .tasks
                .values()
                .filter(|task| self.pids.nr(&self.ns.pid_for_children, task.process.pid.0) != 1)
                .map(|task| task.signals.group().clone())
                .collect()
        };
        let mut done: Vec<Arc<crate::signal::Signals>> = Vec::new();
        for group in groups {
            if !done.iter().any(|g| Arc::ptr_eq(g, &group)) {
                group.send_kernel(sig);
                done.push(group);
            }
        }
    }

    /// `drop_caches_sysctl_handler`'s note of who asked.
    fn drop_caches(&self, opener: u32, value: u32) {
        let comm = self.state.upgrade().and_then(|state| {
            let state = state.read_or_recover();
            Some(state.tasks.get(&TaskId(opener))?.comm.name().to_vec())
        });
        let comm = String::from_utf8_lossy(&comm.unwrap_or_default()).into_owned();
        let pid = self.tgid_of(opener).unwrap_or(0);
        let text = format!("{comm} ({pid}): drop_caches: {value}");
        self.printk.log(crate::printk::Level::Info, &text);
    }

    /// The open files of every task, each counted once (`nr_files`).
    fn nr_files(&self) -> usize {
        let Some(state) = self.state.upgrade() else {
            return 0;
        };
        let tables: Vec<_> = {
            let state = state.read_or_recover();
            state
                .tasks
                .values()
                .map(|task| task.files.clone())
                .collect()
        };
        let mut seen = std::collections::HashSet::new();
        for table in tables {
            for (_, file, _) in table.lock_or_recover().iter() {
                seen.insert(Arc::as_ptr(file) as usize);
            }
        }
        seen.len()
    }

    /// How many tasks run, and how many were ever started (`total_forks`).
    fn task_counts(&self) -> (usize, u32) {
        self.state.upgrade().map_or((0, 0), |state| {
            let state = state.read_or_recover();
            (state.tasks.len(), state.last_id)
        })
    }

    /// The pid namespace of the task `tid`, which the system files of pids show it: its
    /// process's (`task_active_pid_ns(current)`). The embedder, and a task gone meanwhile, see
    /// the initial one.
    fn pid_ns_of(&self, tid: u32) -> Arc<PidNs> {
        let ns = self.state.upgrade().and_then(|state| {
            let state = state.read_or_recover();
            Some(state.tasks.get(&TaskId(tid))?.process.pid_ns.clone())
        });
        ns.unwrap_or_else(|| self.ns.pid_for_children.clone())
    }
}
