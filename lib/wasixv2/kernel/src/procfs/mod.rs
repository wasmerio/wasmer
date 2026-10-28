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

use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use wasixv2_host::Host;
use wasixv2_syscalls::TaskId;
use wasixv2_vfs::{Clock, Filesystem, FsInfo, FsResult, NAME_MAX, Node, StatFs, Timestamp, Vfs};

use crate::{
    Kernel, Process, TaskTable,
    file::FdTable,
    fs::{FsContext, HostClock},
    ident::Comm,
    mm::Mm,
    ns::NsProxy,
    nsfs::Nsfs,
    process::Tree,
    sched::Sched,
    signal::TaskSignals,
};

/// `PROC_SUPER_MAGIC`.
const PROC_SUPER_MAGIC: u32 = 0x9fa0;

impl Kernel {
    /// A process filesystem for this kernel, to mount at `/proc`.
    pub fn procfs(&self) -> Arc<dyn Filesystem> {
        let source = Source {
            state: Arc::downgrade(&self.state),
            tree: Arc::downgrade(&self.procs.tree),
            vfs: Arc::downgrade(&self.vfs),
            host: self.host.clone(),
            ns: self.init_ns.clone(),
            nsfs: self.pseudo.nsfs.clone(),
            mounted: HostClock(self.host.clone()).now(),
        };
        Arc::new(ProcFs {
            source: Arc::new(source),
        })
    }
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
    /// The namespaces the system files show: the initial ones.
    ns: Arc<NsProxy>,
    /// Where `/proc/<pid>/ns` links lead.
    nsfs: Nsfs,
    /// Every node's timestamps: when the filesystem was made.
    mounted: Timestamp,
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
}

impl Source {
    fn process(&self, pid: u32) -> Option<ProcessInfo> {
        let tree = self.tree.upgrade()?;
        let tree = tree.lock().unwrap();
        let node = tree.nodes.get(&pid)?;
        Some(ProcessInfo {
            process: node.process.clone(),
            parent: node.parent,
            exit_signal: node.exit_signal,
            zombie: node.is_zombie(),
        })
    }

    /// Every process, zombies included, lowest pid first.
    fn pids(&self) -> Vec<u32> {
        let Some(tree) = self.tree.upgrade() else {
            return Vec::new();
        };
        let mut pids: Vec<u32> = tree.lock().unwrap().nodes.keys().copied().collect();
        pids.sort_unstable();
        pids
    }

    /// The threads of process `pid`, lowest tid first.
    fn threads(&self, pid: u32) -> Vec<u32> {
        let Some(state) = self.state.upgrade() else {
            return Vec::new();
        };
        let state = state.read().unwrap();
        let mut tids: Vec<u32> = state
            .tasks
            .iter()
            .filter(|(_, task)| task.process.pid.0 == pid)
            .map(|(tid, _)| tid.0)
            .collect();
        tids.sort_unstable();
        tids
    }

    /// The task `tid` of process `pid`, or for `None` the process's leader (its first task
    /// left, once the leader exited).
    fn task(&self, pid: u32, tid: Option<u32>) -> Option<TaskInfo> {
        let tid = match tid {
            Some(tid) => tid,
            None => *self.threads(pid).first()?,
        };
        let state = self.state.upgrade()?;
        let state = state.read().unwrap();
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
        })
    }

    /// Renames the task `tid` of process `pid`, or its leader (`comm_write`).
    fn set_comm(&self, pid: u32, tid: Option<u32>, name: &[u8]) -> Option<()> {
        let tid = match tid {
            Some(tid) => tid,
            None => *self.threads(pid).first()?,
        };
        let state = self.state.upgrade()?;
        let mut state = state.lock().unwrap();
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

    /// The thread group of the task `tid`.
    fn tgid_of(&self, tid: u32) -> Option<u32> {
        let state = self.state.upgrade()?;
        let state = state.read().unwrap();
        Some(state.tasks.get(&TaskId(tid))?.process.pid.0)
    }

    /// How many tasks run, and the last pid handed out.
    fn task_counts(&self) -> (usize, u32) {
        self.state.upgrade().map_or((0, 0), |state| {
            let state = state.read().unwrap();
            (state.tasks.len(), state.last_id)
        })
    }
}
