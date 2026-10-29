//! What the hierarchy's files do with tasks: list them (`css_task_iter`), move them
//! (`cgroup_attach_task`), kill them (`cgroup_kill`), and add up what they use.

use std::{collections::HashSet, sync::Arc};

use wasixv2_syscalls::{Errno, TaskId};
use wasixv2_vfs::{Attr, FileType, MAY_WRITE};

use super::{Cgroup, Cpu, Hierarchy, fs::Source};
use crate::{TaskTable, ns::PidNs};

/// What a control file was opened with (`cgroup_file_open`): the opener's credentials
/// (`f_cred`), its cgroup namespace's root (`ctx->ns`) and its pid namespace. The embedder,
/// which is no task, is in the initial namespaces with every privilege.
pub(crate) struct Opener {
    task: Option<TaskId>,
    cred: Option<wasixv2_vfs::Cred>,
    pub(crate) ns_root: Arc<Cgroup>,
    pub(crate) ns_is_init: bool,
    pid_ns: Option<Arc<PidNs>>,
}

impl Opener {
    pub(crate) fn of(source: &Source, task: u32) -> Self {
        let root = source.hierarchy.root().clone();
        let found = source.state.upgrade().and_then(|state| {
            let state = state.read_or_recover();
            let task = state.tasks.get(&TaskId(task))?;
            let ns = task.ns.as_ref()?;
            Some((
                task.process.cred().fs(),
                ns.cgroup.root.clone(),
                task.process.pid_ns.clone(),
            ))
        });
        match found {
            Some((cred, ns_root, pid_ns)) => Self {
                task: Some(TaskId(task)),
                cred: Some(cred),
                ns_is_init: ns_root.id == root.id,
                ns_root,
                pid_ns: Some(pid_ns),
            },
            None => Self {
                task: None,
                cred: None,
                ns_root: root,
                ns_is_init: true,
                pid_ns: None,
            },
        }
    }

    /// The pid of the task `id` in the opener's pid namespace, 0 where it has none
    /// (`task_pid_vnr`).
    pub(crate) fn pid_nr(&self, state: &TaskTable, id: u32) -> i32 {
        let state = state.read_or_recover();
        match &self.pid_ns {
            Some(ns) => state.pids.nr(ns, id),
            None => id as i32,
        }
    }

    /// `find_task_by_vpid` in the opener's pid namespace.
    fn find(&self, nr: i32) -> Option<u32> {
        match &self.pid_ns {
            Some(ns) => ns.find(nr),
            None => u32::try_from(nr).ok(),
        }
    }

    /// `cgroup_may_write` as the opener.
    fn may_write(&self, hierarchy: &Hierarchy, cgroup: &Cgroup) -> Result<(), Errno> {
        may_write_procs(hierarchy, cgroup, self.cred.as_ref())
    }
}

/// `cgroup_may_write`: whether `cred` may write `cgroup`'s `cgroup.procs`; the embedder (no
/// credentials) may.
pub(crate) fn may_write_procs(
    hierarchy: &Hierarchy,
    cgroup: &Cgroup,
    cred: Option<&wasixv2_vfs::Cred>,
) -> Result<(), Errno> {
    let Some(cred) = cred else {
        return Ok(());
    };
    let file = super::fs::file_index("cgroup.procs");
    let node = hierarchy.lock().file_attr(cgroup, file);
    let attr = Attr {
        ino: Hierarchy::file_ino(cgroup, file),
        file_type: FileType::Regular,
        mode: node.mode,
        nlink: 1,
        uid: node.uid,
        gid: node.gid,
        rdev: Default::default(),
        size: 0,
        blocks: 0,
        blksize: 4096,
        atime: node.atime,
        mtime: node.mtime,
        ctime: node.ctime,
        btime: None,
        dev: None,
    };
    wasixv2_vfs::generic_permission(cred, &attr, MAY_WRITE)
}

/// The ids `cgroup.procs` (the processes with a task in `cgroup` or a threaded cgroup of its
/// domain, by their leaders) or `cgroup.threads` (the tasks in `cgroup` itself) lists: tasks
/// that did not exit, lowest first.
pub(crate) fn members(
    state: &TaskTable,
    hierarchy: &Hierarchy,
    cgroup: &Arc<Cgroup>,
    threads: bool,
) -> Vec<u32> {
    let state = state.read_or_recover();
    let tree = hierarchy.lock();
    let mut ids: Vec<u32> = state
        .tasks
        .iter()
        .filter(|(_, task)| !task.css.exited())
        .filter(|(_, task)| {
            let at = task.css.cgroup();
            match threads {
                true => at.id == cgroup.id,
                false => tree.dom(&at).id == cgroup.id,
            }
        })
        .map(|(id, task)| match threads {
            true => id.0,
            false => task.process.pid.0,
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The CPU time of `cgroup`'s subtree: what tasks used while they were in it.
pub(crate) fn cpu(state: &TaskTable, hierarchy: &Hierarchy, cgroup: &Arc<Cgroup>) -> Cpu {
    let state = state.read_or_recover();
    let tree = hierarchy.lock();
    let live = state
        .tasks
        .values()
        .filter(|task| task.css.cgroup().descends_from(cgroup))
        .map(|task| task.css.cpu());
    live.fold(tree.cpu_retired(cgroup), Cpu::add)
}

/// The bytes the address spaces of `cgroup`'s subtree hold, each counted once.
pub(crate) fn memory_current(
    state: &TaskTable,
    hierarchy: &Hierarchy,
    cgroup: &Arc<Cgroup>,
) -> u64 {
    let _ = hierarchy;
    let state = state.read_or_recover();
    let mut seen = HashSet::new();
    state
        .tasks
        .values()
        .filter(|task| !task.css.exited() && task.css.cgroup().descends_from(cgroup))
        .filter(|task| seen.insert(task.mm.id()))
        .map(|task| task.mm.size())
        .sum()
}

/// `__cgroup_kill`: SIGKILL to every task of `cgroup`'s subtree.
pub(crate) fn kill(state: &TaskTable, hierarchy: &Hierarchy, cgroup: &Arc<Cgroup>) {
    let _ = hierarchy;
    let groups: Vec<_> = {
        let state = state.read_or_recover();
        state
            .tasks
            .values()
            .filter(|task| !task.css.exited() && task.css.cgroup().descends_from(cgroup))
            .map(|task| task.signals.group().clone())
            .collect()
    };
    let mut done = Vec::new();
    for group in groups {
        if done.iter().any(|g| Arc::ptr_eq(g, &group)) {
            continue;
        }
        group.send_kernel(wasixv2_abi::Signal::SIGKILL.0);
        done.push(group);
    }
}

/// `cgroup_freeze`: `cgroup.freeze` of `cgroup` becomes `freeze`, and the tasks of every
/// cgroup whose effective state changed are frozen or thawed (`cgroup_freeze_task`). The task
/// table stays locked, so no task forks or moves meanwhile.
pub(crate) fn freeze(state: &TaskTable, hierarchy: &Hierarchy, cgroup: &Arc<Cgroup>, freeze: bool) {
    let state = state.read_or_recover();
    let mut tree = hierarchy.lock();
    let changed = tree.freeze(cgroup, freeze);
    if changed.is_empty() {
        return;
    }
    for task in state.tasks.values().filter(|task| !task.css.exited()) {
        let at = task.css.cgroup().id;
        if let Some(&(_, freeze)) = changed.iter().find(|(id, _)| *id == at) {
            task.signals.freeze(freeze);
        }
    }
}

/// `__cgroup_procs_write`: moves the process (`threadgroup`) or the thread `text` names, 0 for
/// the opener, to `dst`, checked with what the opener had (`cgroup_attach_permissions`).
pub(crate) fn attach(
    state: &TaskTable,
    source: &Source,
    opener: &Opener,
    dst: &Arc<Cgroup>,
    text: &str,
    threadgroup: bool,
) -> Result<(), Errno> {
    // Linux kernel/cgroup/cgroup.c:cgroup_procs_write_start uses kstrtoint
    // with base 0; lookup stays in the pid namespace captured by cgroup_file_open.
    let nr = super::fs::parse_i32(text)? as i32;
    if nr < 0 {
        return Err(Errno::EINVAL);
    }
    let hierarchy = &source.hierarchy;
    let state = state.lock_or_recover();
    let target = match nr {
        0 => opener.task.ok_or(Errno::ESRCH)?,
        nr => TaskId(opener.find(nr).ok_or(Errno::ESRCH)?),
    };
    let task = state.tasks.get(&target).ok_or(Errno::ESRCH)?;
    let leader = task.process.pid;
    let src = match threadgroup {
        true => state
            .tasks
            .get(&leader)
            .unwrap_or(task)
            .css
            .cgroup()
            .clone(),
        false => task.css.cgroup().clone(),
    };
    // `cgroup_procs_write_permission`: the opener may write the common ancestor's
    // `cgroup.procs`, and with `nsdelegate` sees both cgroups from its namespace.
    let common = Cgroup::common(&src, dst);
    opener.may_write(hierarchy, &common)?;
    let mut tree = hierarchy.lock();
    if tree.flags.nsdelegate
        && (!src.descends_from(&opener.ns_root) || !dst.descends_from(&opener.ns_root))
    {
        return Err(Errno::ENOENT);
    }
    tree.may_attach(&src, dst, threadgroup)?;
    let process = task.process.clone();
    let moving: Vec<TaskId> = match threadgroup {
        true => state
            .tasks
            .iter()
            .filter(|(_, t)| Arc::ptr_eq(&t.process, &process))
            .map(|(&id, _)| id)
            .collect(),
        false => vec![target],
    };
    for id in moving {
        let task = state.tasks.get(&id).expect("listed above");
        // `cgroup_migrate_add_task`: an exiting task stays where it is.
        if !task.css.exited()
            && let Some(freeze) = task.css.migrate(&mut tree, dst)
        {
            task.signals.freeze(freeze);
        }
    }
    Ok(())
}
