//! The process filesystem (`fs/proc`): the kernel's live state as files, following
//! `Documentation/filesystems/proc.rst`.
//!
//! Every node is computed from the kernel when it is looked up or opened, never stored: a
//! directory per process and per thread, their descriptors as magic links that jump to the
//! open file's object (`proc_fd_link`), and the system-wide files. A file's contents are
//! produced when it is opened and read from that snapshot, as `seq_file` does for the first
//! read.

mod node;
mod text;

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
        let source = Source {
            state: Arc::downgrade(&self.state),
            tree: Arc::downgrade(&self.procs.tree),
            vfs: Arc::downgrade(&self.vfs),
            host: self.host.clone(),
            pid_ns,
            pids: self.pids.clone(),
            ns: self.init_ns.clone(),
            nsfs: self.pseudo.nsfs.clone(),
            filesystems: self.filesystems.clone(),
            mounted: HostClock(self.host.clone()).now(),
            nr_open: self.nr_open.clone(),
            taint: self.taint.clone(),
            pipe_max_size: self.pipe_max_size.clone(),
            vm: self.vm.clone(),
        };
        Arc::new(ProcFs {
            source: Arc::new(source),
        })
    }
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
    /// Every node's timestamps: when the filesystem was made.
    mounted: Timestamp,
    /// `fs.nr_open`.
    nr_open: Arc<NrOpen>,
    /// `kernel.tainted`.
    taint: Arc<Taint>,
    /// `fs.pipe-max-size`.
    pipe_max_size: Arc<PipeMaxSize>,
    /// The `vm` tunables.
    vm: Arc<VmSysctls>,
}

/// A process as the tree has it.
struct ProcessInfo {
    process: Arc<Process>,
    parent: u32,
    exit_signal: i32,
    zombie: bool,
}

/// A task's shared parts.
struct TaskInfo {
    files: Arc<Mutex<FdTable>>,
    fs: Arc<Mutex<FsContext>>,
    signals: Arc<TaskSignals>,
    /// The address space (`get_task_mm`).
    mm: Arc<Mm>,
    comm: Comm,
    sched: Sched,
    ns: Arc<NsProxy>,
    cpu: CpuTimes,
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
            files: task.files.clone(),
            fs: task.fs.clone(),
            signals: task.signals.clone(),
            mm: task.mm.clone(),
            comm: task.comm,
            sched: task.sched.clone(),
            ns: task.ns.clone(),
            cpu: task.cpu.times(),
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
            Some(state.tasks.get(&TaskId(tid))?.ns.clone())
        });
        ns.unwrap_or_else(|| self.ns.clone())
    }

    /// The credentials of the task `tid`, which opens a file (`f_cred`); none for the embedder
    /// or a task gone meanwhile.
    fn cred_of(&self, tid: u32) -> Option<crate::cred::Cred> {
        let state = self.state.upgrade()?;
        let state = state.read_or_recover();
        Some(state.tasks.get(&TaskId(tid))?.process.cred())
    }

    /// The user namespace of the task `tid`, which opens a file and sees its ids through it
    /// (`seq_user_ns`); without the task, the one that owns the instance's pid namespace.
    fn user_ns_of(&self, tid: u32) -> Arc<UserNs> {
        let ns = self.cred_of(tid).map(|cred| cred.user_ns);
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
            let cred = task.process.cred();
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
