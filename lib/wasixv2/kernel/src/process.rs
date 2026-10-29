//! Processes, after Linux's `kernel/fork.c`, `kernel/exit.c`, `fs/exec.c` and the process-group
//! half of `kernel/sys.c`: the process tree, `clone` for new processes (fork and vfork),
//! `execve`, zombies and the `wait` family, reparenting, SIGCHLD, process groups, sessions and
//! pidfds.
//!
//! vfork runs the child in its parent's instance: `clone` asks the execution layer to switch
//! the instance to the child ([`GuestAction::SwitchTask`]), and the child's exit or `execve`
//! switches it back, as the kernel tracks in [`Tree::vforks`]. fork asks the execution layer to
//! copy the caller's memory and stack into a new instance for the child
//! ([`GuestAction::Fork`]).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use wasixv2_abi::{
    AtFlags, CloneArgs, CloneArgsSize, CloneFlags, ConstCloneArgsPtr, ConstSiginfoPtr, Cstr,
    CstrArray, GuestLayout, GuestPtr, GuestPtrMut, Idtype, OpenFlags, Pid, Rusage, RusagePtr,
    S32Ptr, SiCode, Siginfo, SiginfoPtr, Signal, Ulong, Uptr, Usize, WaitOptions,
};
use wasixv2_syscalls::{
    BINPRM_BUF_SIZE, Binfmt, Caller, CpuAccount, CpuTimes, CstrError, Errno, Fd, GuestAction,
    Image, LocalFuture, Resume, SysErr, SysResult, TaskId, VforkParent, WorkFlag, read_cstr,
    read_value, write_value,
};
use wasixv2_vfs::{Cap, FileType, LookupFlags, MAY_EXEC, OpenHow};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel, Process, State, Task,
    file::{Backing, FdTable, FileObject, Io, OpenFile},
    fs::FdReservation,
    ident::{Comm, Ident},
    ns::PidNs,
    pid::{PidType, Pids},
    signal::{Signals, user_info},
    sync::MutexExt,
    thread,
    work::Work,
};

/// `MAX_ARG_STRLEN`: the longest single argument or environment string.
const MAX_ARG_STRLEN: usize = 32 * 4096;
/// `ARG_MAX` as Linux computes it from the default 8 MiB stack limit: a quarter of it.
const ARG_MAX: usize = 2 * 1024 * 1024;
/// `MAX_ARG_STRINGS`.
const MAX_ARG_STRINGS: usize = 0x7FFF_FFFF;
/// How many times a script may hand over to its interpreter, which may be a script itself, as
/// `exec_binprm`'s five levels of rewrites.
const MAX_REWRITES: usize = 5;
const WASM_MAGIC: &[u8] = b"\0asm";

/// The kernel's [`Binfmt`] until the embedder gives it one: any file that starts with wasm's
/// magic is an image; what serves it is found only once it runs.
pub(crate) struct WasmMagic;

impl Binfmt for WasmMagic {
    fn probe(&self, header: &[u8]) -> Result<(), Errno> {
        match header.starts_with(WASM_MAGIC) {
            true => Ok(()),
            false => Err(Errno::ENOEXEC),
        }
    }
}

/// `__WCLONE`: wait only for children that do not report with SIGCHLD.
const WCLONE: u32 = 0x8000_0000;
const WAIT_KNOWN: u32 = WaitOptions::WNOHANG.0
    | WaitOptions::WSTOPPED.0
    | WaitOptions::WEXITED.0
    | WaitOptions::WCONTINUED.0
    | WaitOptions::WNOWAIT.0
    | WaitOptions::__WNOTHREAD.0
    | WaitOptions::__WALL.0
    | WCLONE;

/// `PIDFD_NONBLOCK` (`O_NONBLOCK`).
const PIDFD_NONBLOCK: u32 = OpenFlags::O_NONBLOCK.0;

#[derive(Default)]
pub(crate) struct Procs {
    pub(crate) tree: Arc<Mutex<Tree>>,
    /// Woken whenever a process exits; waiters and pidfd pollers recheck (`wait_chldexit`).
    pub(crate) changed: Arc<WaitQueue>,
    /// Woken whenever a fork child leaves [`Tree::fork_vforks`] (`complete_vfork_done`).
    vfork_done: WaitQueue,
}

#[derive(Default)]
pub(crate) struct Tree {
    pub(crate) nodes: HashMap<u32, Node>,
    /// vfork children running in their parent's instance, to the parent task that waits.
    pub(crate) vforks: HashMap<TaskId, TaskId>,
    /// vfork children running in their parent's instance whose parent waits no more because
    /// its image went on in another instance (another thread's `execve` gave the leader the new
    /// image): the instance ends with the child ([`Resume::Gone`]). To the parent task.
    pub(crate) detached: HashMap<TaskId, TaskId>,
    /// The pid each vfork child has as its parent sees it, which vfork returns there
    /// ([`VforkParent::pid`]), until the parent goes back to its instance.
    pub(crate) vfork_pids: HashMap<TaskId, i32>,
    /// Fork children whose parent waits until they are done with their copy of its memory
    /// (`CLONE_VFORK` without `CLONE_VM`, Linux's `vfork_done`), to that parent.
    pub(crate) fork_vforks: HashMap<TaskId, TaskId>,
    /// The CPU time of every process reaped so far, which the machine's counts with the live
    /// ones' (`/proc/stat`).
    pub(crate) reaped: CpuTimes,
    /// The kernel's pid numbers, which a process in the tree holds for its thread group,
    /// process group and session.
    pub(crate) pids: Arc<Pids>,
}

impl Tree {
    /// Takes `pid` out of the tree, as it is reaped (`release_task`): its thread group's id,
    /// process group and session hold its pids no more.
    pub(crate) fn reap(&mut self, pid: u32) -> Option<Node> {
        let node = self.nodes.remove(&pid)?;
        self.reaped = self.reaped.saturating_add(node.process.cpu.times());
        let (pgid, sid) = {
            let ident = node.process.ident.lock_or_recover();
            (ident.pgid, ident.sid)
        };
        self.pids.detach(pid, PidType::Tgid);
        self.pids.detach(pgid, PidType::Pgid);
        self.pids.detach(sid, PidType::Sid);
        Some(node)
    }

    /// Moves `process`, in the tree, to the process group `pgid` and, with `sid`, the session
    /// (`change_pid`).
    fn set_pgrp(&self, ident: &mut Ident, pgid: u32, sid: Option<u32>) {
        self.pids.attach(pgid, PidType::Pgid);
        self.pids
            .detach(std::mem::replace(&mut ident.pgid, pgid), PidType::Pgid);
        if let Some(sid) = sid {
            self.pids.attach(sid, PidType::Sid);
            self.pids
                .detach(std::mem::replace(&mut ident.sid, sid), PidType::Sid);
        }
    }

    /// The vfork child `child` is done with its parent's instance: the parent the instance
    /// goes back to, which returns from vfork unless [`vfork_parent_resumes`] finds its image
    /// ended, or is gone from the instance.
    pub(crate) fn end_vfork(&mut self, child: TaskId) -> Option<VforkParent> {
        let (task, resumes) = match self.vforks.remove(&child) {
            Some(task) => (task, Resume::Returns),
            None => (self.detached.remove(&child)?, Resume::Gone),
        };
        let pid = self.vfork_pids.remove(&child).unwrap_or_default();
        Some(VforkParent { task, resumes, pid })
    }

    /// The vfork child running in the instance of `parent`, which waits for it.
    pub(crate) fn waited_for_by(&self, parent: TaskId) -> Option<TaskId> {
        let mut children = self.vforks.iter();
        children
            .find(|&(_, &waiting)| waiting == parent)
            .map(|(&child, _)| child)
    }
}

pub(crate) struct Node {
    pub(crate) process: Arc<Process>,
    /// 0 for none: the first process, or an orphan with nobody to reap it.
    pub(crate) parent: u32,
    /// The signal the parent gets when the process exits (`CSIGNAL`).
    pub(crate) exit_signal: i32,
    /// For `setpgid`'s `EACCES`.
    did_exec: bool,
    /// The signal whose default action killed the process.
    killed_by: Option<i32>,
    /// Set once the process exited; it is a zombie until reaped.
    exit: Option<Exit>,
}

impl Node {
    /// A zombie: exited, not reaped.
    pub(crate) fn is_zombie(&self) -> bool {
        self.exit.is_some()
    }
}

/// The file `execve` runs: where it is, and what of it decides the new image's credentials.
struct Binary {
    path: wasixv2_vfs::Path,
    mode: u32,
    owner: (u32, u32),
    /// On a `nosuid` mount, whose set-user-ID and set-group-ID bits do not apply.
    nosuid: bool,
    /// Its size when it was opened.
    size: u64,
}

/// How a process ended, as `waitid` reports it.
#[derive(Clone, Copy)]
struct Exit {
    code: SiCode,
    status: i32,
    /// The process's real uid when it exited.
    uid: u32,
}

impl Procs {
    pub(crate) fn new(pids: Arc<Pids>) -> Self {
        Self {
            tree: Arc::new(Mutex::new(Tree {
                pids,
                ..Tree::default()
            })),
            changed: Arc::default(),
            vfork_done: WaitQueue::default(),
        }
    }

    /// Enters `process` into the tree under `parent` (0 for none), where it holds the pids of
    /// its thread group, process group and session.
    pub(crate) fn add(&self, process: Arc<Process>, parent: u32, exit_signal: i32) {
        let pid = process.pid.0;
        let (pgid, sid) = {
            let ident = process.ident.lock_or_recover();
            (ident.pgid, ident.sid)
        };
        let mut tree = self.tree.lock_or_recover();
        tree.pids.attach(pid, PidType::Tgid);
        tree.pids.attach(pgid, PidType::Pgid);
        tree.pids.attach(sid, PidType::Sid);
        tree.nodes.insert(
            pid,
            Node {
                process,
                parent,
                exit_signal,
                did_exec: false,
                killed_by: None,
                exit: None,
            },
        );
    }

    /// The process `pid`, zombies included.
    pub(crate) fn find(&self, pid: u32) -> Option<Arc<Process>> {
        let tree = self.tree.lock_or_recover();
        tree.nodes.get(&pid).map(|node| node.process.clone())
    }
}

impl Tree {
    /// The process group and session of the process `pid`.
    fn pgrp_sid(&self, pid: u32) -> Option<(u32, u32)> {
        let ident = self.nodes.get(&pid)?.process.ident.lock_or_recover();
        Some((ident.pgid, ident.sid))
    }

    /// `will_become_orphaned_pgrp`: no live process of the group `pgid` but `ignored` has a
    /// parent in another group of the same session, so nothing outside the group can bring it
    /// back from a stop (POSIX's orphaned process group). Children of init, or of nobody, keep
    /// no group from being orphaned.
    pub(crate) fn will_become_orphaned_pgrp(&self, pgid: u32, ignored: Option<u32>) -> bool {
        !self.nodes.iter().any(|(&pid, node)| {
            if Some(pid) == ignored || node.is_zombie() || matches!(node.parent, 0 | 1) {
                return false;
            }
            let Some((pgrp, sid)) = self.pgrp_sid(pid) else {
                return false;
            };
            pgrp == pgid
                && self
                    .pgrp_sid(node.parent)
                    .is_some_and(|(parent_pgrp, parent_sid)| {
                        parent_pgrp != pgid && parent_sid == sid
                    })
        })
    }

    /// `kill_orphaned_pgrp`'s test for the process group and session `member` of a process
    /// whose link to `parent`, its parent's group and session, is going: the exiting process
    /// itself (`ignored`), or a child of an exiting parent, handed to its reaper already. The
    /// group, if it is orphaned now, is for the caller to check for stopped jobs.
    fn orphaned_by(
        &self,
        member: (u32, u32),
        parent: (u32, u32),
        ignored: Option<u32>,
    ) -> Option<u32> {
        let ((pgrp, sid), (parent_pgrp, parent_sid)) = (member, parent);
        (parent_pgrp != pgrp && parent_sid == sid && self.will_become_orphaned_pgrp(pgrp, ignored))
            .then_some(pgrp)
    }

    fn children(&self, pid: u32) -> impl Iterator<Item = (&u32, &Node)> {
        self.nodes
            .iter()
            .filter(move |(_, node)| node.parent == pid)
    }

    /// `find_new_reaper`: the nearest living ancestor in the same pid namespace that set
    /// `PR_SET_CHILD_SUBREAPER`, else the namespace's init (`child_reaper`: in the initial
    /// namespace the first process, the kernel's init when it has one), else nobody. A dying
    /// init has no reaper: its namespace is empty by then.
    fn reaper(&self, dying: u32) -> u32 {
        let Some(node) = self.nodes.get(&dying) else {
            return 0;
        };
        let ns = &node.process.pid_ns;
        let mut at = node.parent;
        while let Some(node) = self.nodes.get(&at) {
            // A parent in another pid namespace was injected by `setns` and `fork`.
            if node.process.pid_ns.level() != ns.level() {
                break;
            }
            if node.exit.is_none() && node.process.ident.lock_or_recover().child_subreaper() {
                return at;
            }
            at = node.parent;
        }
        match ns.reaper() {
            Some(init)
                if init != dying && self.nodes.get(&init).is_some_and(|n| n.exit.is_none()) =>
            {
                init
            }
            _ => 0,
        }
    }

    /// The pid namespace of the process `pid`, while it is in the tree.
    fn pid_ns_of(&self, pid: u32) -> Option<Arc<PidNs>> {
        Some(self.nodes.get(&pid)?.process.pid_ns.clone())
    }

    /// `task_pid_nr_ns(id, task_active_pid_ns(viewer))`: the id `id` as the process `viewer`
    /// sees it.
    fn nr_for(&self, viewer: u32, id: u32) -> Pid {
        self.pid_ns_of(viewer).map_or(0, |ns| self.pids.nr(&ns, id))
    }
}

/// `action` as the instance gets it from [`Control::take_pending`]: going back to a vfork
/// parent, it says whether the parent returns from vfork. Not when its image ended by now: a
/// group exit, or another thread's `execve` (`de_thread`), killed it in `wait_for_vfork_done`
/// (`zap_other_threads`); and not when it is gone from the instance.
///
/// [`Control::take_pending`]: wasixv2_syscalls::Control::take_pending
pub(crate) fn vfork_parent_resumes(state: &State, action: GuestAction) -> GuestAction {
    let ended = |task: TaskId| state.tasks.get(&task).is_some_and(|t| t.work.ends_image());
    let resumes = |parent: VforkParent| VforkParent {
        resumes: match parent.resumes {
            Resume::Gone => Resume::Gone,
            _ if ended(parent.task) => Resume::Dies,
            _ => Resume::Returns,
        },
        ..parent
    };
    match action {
        GuestAction::VforkReturn { parent, child } => GuestAction::VforkReturn {
            parent: resumes(parent),
            child,
        },
        GuestAction::Exec {
            vfork_parent: Some(parent),
        } => GuestAction::Exec {
            vfork_parent: Some(resumes(parent)),
        },
        action => action,
    }
}

/// The `siginfo` of SIGCHLD and `waitid`: `si_pid` (the child's pid as the parent sees it),
/// `si_uid` (the child's real uid), `si_status`.
fn child_info(signo: i32, pid: Pid, exit: Exit) -> Siginfo {
    let mut info = Siginfo {
        si_signo: signo,
        si_errno: 0,
        si_code: exit.code.0,
        sifields: [0; 116],
    };
    info.sifields[0..4].copy_from_slice(&pid.to_le_bytes());
    info.sifields[4..8].copy_from_slice(&exit.uid.to_le_bytes());
    info.sifields[8..12].copy_from_slice(&exit.status.to_le_bytes());
    info
}

/// Which children a `waitid` asks about.
#[derive(Clone, Copy)]
enum Which {
    All,
    Pid(u32),
    Pgid(u32),
}

impl Kernel {
    /// `task_tgid_vnr(current->real_parent)`: 0 for a parent outside the caller's pid
    /// namespace, as its init's is.
    pub(crate) fn sys_getppid(&self, caller: &Caller<'_>) -> Pid {
        let process = self.process(caller.task());
        let tree = self.procs.tree.lock_or_recover();
        let parent = tree.nodes.get(&process.pid.0).map_or(0, |node| node.parent);
        tree.pids.nr(&process.pid_ns, parent)
    }

    /// The vfork child running in `parent`'s instance is done with it: `parent` waits no more.
    pub(crate) fn vfork_done(&self, parent: TaskId) {
        let signals = {
            let state = self.state.read_or_recover();
            state.tasks.get(&parent).map(|task| task.signals.clone())
        };
        if let Some(signals) = signals {
            signals.set_vfork_child(None);
        }
    }

    /// `wait_for_vfork_done` is killable: when the vfork parent of `child`, the task its
    /// instance runs, has a fatal signal pending, the parent's thread group dies now, as
    /// though the parent had taken it, while the child goes on. The instance goes back to the
    /// parent only once the child ends, to unwind it. A parent whose exit is queued already
    /// has nothing left to take: a group exit reached it, or another thread's `execve` killed
    /// it alone (`zap_other_threads`; `do_group_exit` with `group_exec_task` set ends only the
    /// thread that takes the SIGKILL).
    pub(crate) fn kill_vfork_parent(&self, child: TaskId) {
        let Some(parent) = self
            .procs
            .tree
            .lock_or_recover()
            .vforks
            .get(&child)
            .copied()
        else {
            return;
        };
        let signals = {
            let state = self.state.read_or_recover();
            let task = state.tasks.get(&parent);
            let alive = task.filter(|task| task.work.exit().is_none());
            alive.map(|task| task.signals.clone())
        };
        if let Some(sig) = signals.and_then(|signals| signals.fatal_pending()) {
            self.force_fatal_signal(parent, sig);
        }
    }

    /// `complete_vfork_done`, from `task`'s `mm_release`: a fork child that its parent waits
    /// for (`CLONE_VFORK` without `CLONE_VM`) is done with the memory it was made with, as it
    /// exits, its `execve` replaces the memory, or it never runs. The parent goes on.
    pub(crate) fn complete_vfork_done(&self, task: TaskId) {
        let parent = self.procs.tree.lock_or_recover().fork_vforks.remove(&task);
        if parent.is_some() {
            self.procs.vfork_done.wake(PollMask::empty());
        }
    }

    /// `wait_for_vfork_done`, killable: after a fork with `CLONE_VFORK`, once the child runs,
    /// `task` waits until the child is done with its copy of the memory
    /// ([`Kernel::complete_vfork_done`]). A fatal signal ends the wait and the thread group,
    /// and the child completes nothing for it any more.
    pub(crate) async fn vfork_wait(&self, task: TaskId) {
        let signals = self.task_signals(task);
        let done = || {
            let tree = self.procs.tree.lock_or_recover();
            (!tree.fork_vforks.values().any(|&parent| parent == task)).then_some(())
        };
        if wait_event(&self.procs.vfork_done, done, &signals.killable())
            .await
            .is_ok()
        {
            return;
        }
        let fatal = signals.fatal_pending();
        self.procs
            .tree
            .lock_or_recover()
            .fork_vforks
            .retain(|_, parent| *parent != task);
        // `complete_signal`: a fatal signal ends the whole group as it arrives.
        if let Some(sig) = fatal {
            self.force_fatal_signal(task, sig);
        }
    }

    /// The tasks that wait for a vfork child running in their instance.
    pub(crate) fn vfork_parents(&self) -> Vec<TaskId> {
        let tree = self.procs.tree.lock_or_recover();
        tree.vforks.values().copied().collect()
    }

    /// Whether `task` waits for a vfork child running in its instance.
    pub(crate) fn vfork_waits(&self, task: TaskId) -> bool {
        let tree = self.procs.tree.lock_or_recover();
        tree.vforks.values().any(|&parent| parent == task)
    }

    /// Whether `task` is a vfork child running in the instance of its parent, which waits or
    /// went on elsewhere.
    pub(crate) fn vfork_child(&self, task: TaskId) -> bool {
        let tree = self.procs.tree.lock_or_recover();
        tree.vforks.contains_key(&task) || tree.detached.contains_key(&task)
    }

    /// Makes the vfork child that `parent` waits for, which runs in `parent`'s instance, leave
    /// an interruptible wait for kernel work: `parent` died, and its instance has its
    /// `exit_mm` to run ([`GuestAction::ExitMm`]).
    pub(crate) fn notify_vfork_child(&self, parent: TaskId) {
        let Some(child) = self.procs.tree.lock_or_recover().waited_for_by(parent) else {
            return;
        };
        let signals = {
            let state = self.state.read_or_recover();
            state.tasks.get(&child).map(|t| t.signals.clone())
        };
        if let Some(signals) = signals {
            signals.group().notify(child);
        }
    }

    /// The signal state of the thread group led by `pid`.
    fn group_signals(&self, pid: u32) -> Option<Arc<Signals>> {
        self.jobs.group_signals(pid)
    }

    /// Records that a signal's default action is killing the process `task` belongs to, so its
    /// parent sees `WIFSIGNALED`.
    pub(crate) fn note_killed(&self, task: TaskId, sig: i32) {
        let pid = self.process(task).pid.0;
        let mut tree = self.procs.tree.lock_or_recover();
        if let Some(node) = tree.nodes.get_mut(&pid)
            && node.exit.is_none()
        {
            node.killed_by.get_or_insert(sig);
        }
    }

    /// `exit_notify`, once per process, when its thread group's exit is decided: makes it a
    /// zombie, hands its children to a reaper, tells the parent, and releases a vfork parent.
    pub(crate) fn process_exited(&self, process: &Arc<Process>, task: TaskId, status: i32) {
        let pid = process.pid.0;
        let (parent, reaper) = {
            let tree = self.procs.tree.lock_or_recover();
            match tree.nodes.get(&pid) {
                Some(node) if node.exit.is_none() => (node.parent, tree.reaper(pid)),
                _ => return,
            }
        };
        self.exit_files(process);
        // `do_exit`'s `disassociate_ctty(1)` for a session leader, after its files closed.
        self.disassociate_ctty(process, true);
        // `do_notify_parent`: how the parent treats SIGCHLD decides whether a zombie stays.
        let parent_signals = (parent != 0).then(|| self.group_signals(parent)).flatten();
        let (ignored, nocldwait) = parent_signals
            .as_ref()
            .map_or((true, true), |signals| signals.child_reaping());
        // The same for the children that are zombies already, under their new parent.
        // A reaper whose tasks are all gone, a pid namespace's init waiting for its namespace to
        // empty, waits for nothing: its zombies are reaped, as the init's ignoring SIGCHLD
        // makes them on Linux.
        let reaper_signals = (reaper != 0).then(|| self.group_signals(reaper)).flatten();
        let reaper_reaps = match &reaper_signals {
            Some(signals) => Some(signals.child_reaping()),
            None => (reaper != 0).then_some((true, true)),
        };
        let (exit, exit_signal, vfork_parent, orphans, inherited, orphaned_pgrps) = {
            let mut tree = self.procs.tree.lock_or_recover();
            // A reaper that went meanwhile is replaced, and it is not known how the new one
            // treats SIGCHLD: its zombies stay, as a parent that waits keeps them.
            let (reaper, reaper_reaps) = match tree.reaper(pid) {
                now if now == reaper => (reaper, reaper_reaps),
                now => (now, None),
            };
            let Some(node) = tree.nodes.get_mut(&pid) else {
                return;
            };
            if node.exit.is_some() {
                return;
            }
            let uid = process.cred().user.real;
            let exit = match node.killed_by {
                Some(sig) => Exit {
                    code: SiCode::CLD_KILLED,
                    status: sig,
                    uid,
                },
                None => Exit {
                    code: SiCode::CLD_EXITED,
                    status: status & 0xff,
                    uid,
                },
            };
            node.exit = Some(exit);
            let exit_signal = node.exit_signal;
            // `do_notify_parent`: the pid as the parent sees it, before the pid can go.
            let pid_for_parent = tree.nr_for(parent, pid);
            let father = tree.pgrp_sid(pid);
            let autoreap = parent_signals.is_none()
                || (exit_signal == Signal::SIGCHLD.0 && (ignored || nocldwait));
            if autoreap {
                tree.reap(pid);
            }
            // `forget_original_parent`: the children go to the reaper, and those that asked for
            // it hear of their parent's death. A zombie among them is reported to the reaper
            // (`reparent_leader`), which reaps it there and then when it ignores SIGCHLD or
            // nobody takes it.
            let children: Vec<u32> = tree.children(pid).map(|(&child, _)| child).collect();
            let mut orphans = Vec::new();
            let mut inherited = Vec::new();
            for &child in &children {
                // Each as its new parent, or itself, sees the other.
                let child_for_reaper = tree.nr_for(reaper, child);
                let parent_for_child = tree.nr_for(child, pid);
                let node = tree.nodes.get_mut(&child).expect("listed above");
                node.parent = reaper;
                // "We don't want people slaying init."
                node.exit_signal = Signal::SIGCHLD.0;
                let pdeathsig = node.process.ident.lock_or_recover().pdeathsig();
                match (node.exit, reaper_reaps) {
                    (Some(_), _) if reaper == 0 => drop(tree.reap(child)),
                    (Some(_), Some((true, _) | (_, true))) => drop(tree.reap(child)),
                    (Some(exit), Some(_)) => inherited.push((child_for_reaper, exit)),
                    (None, _) if pdeathsig != 0 => {
                        orphans.push((child, pdeathsig, parent_for_child));
                    }
                    _ => {}
                }
            }
            // `kill_orphaned_pgrp` for each child, whose parent's group no longer holds its own,
            // and for the process itself, against its parent.
            let mut orphaned_pgrps = Vec::new();
            if let Some(father) = father {
                for child in children.iter().filter_map(|&child| tree.pgrp_sid(child)) {
                    orphaned_pgrps.extend(tree.orphaned_by(child, father, None));
                }
                if let Some(grandfather) = tree.pgrp_sid(parent) {
                    orphaned_pgrps.extend(tree.orphaned_by(father, grandfather, Some(pid)));
                }
            }
            orphaned_pgrps.sort_unstable();
            orphaned_pgrps.dedup();
            (
                exit,
                (exit_signal, pid_for_parent),
                tree.end_vfork(task),
                orphans,
                inherited,
                orphaned_pgrps,
            )
        };
        let (exit_signal, pid_for_parent) = exit_signal;
        for pgid in orphaned_pgrps {
            self.kill_orphaned_pgrp(pgid);
        }
        if let Some(signals) = reaper_signals {
            for (child, exit) in inherited {
                let _ = signals.send_group(child_info(Signal::SIGCHLD.0, child, exit));
            }
        }
        // `group_send_sig_info(pdeath_signal, SEND_SIG_NOINFO)`: from the parent, as `kill`;
        // from outside the child's pid namespace when it has no pid there.
        for (child, sig, parent_pid) in orphans {
            if let Some(signals) = self.group_signals(child) {
                let info = user_info(sig, SiCode::SI_USER, parent_pid, exit.uid);
                let _ = signals.send_from(None, info, parent_pid == 0);
            }
        }
        if let Some(parent) = vfork_parent {
            // A parent gone from the instance waits for nothing.
            if parent.resumes != Resume::Gone {
                self.vfork_done(parent.task);
            }
            let action = GuestAction::VforkReturn {
                parent,
                child: task,
            };
            self.queue(task, action);
        }
        if let Some(signals) = parent_signals
            && exit_signal != 0
            && !ignored
        {
            // A realtime exit signal its parent has no room for is lost.
            let _ = signals.send_group(child_info(exit_signal, pid_for_parent, exit));
        }
        self.procs.changed.wake(PollMask::empty());
        self.pid_ns_progress();
    }

    /// `kill_orphaned_pgrp`'s end: the process group `pgid` was just orphaned, and if one of its
    /// processes is stopped, nothing outside could ever continue it: every process gets SIGHUP,
    /// then SIGCONT (`SEND_SIG_PRIV`), so the stopped ones go on and, by default, die.
    fn kill_orphaned_pgrp(&self, pgid: u32) {
        let groups = self.pgrp_groups(pgid);
        if !groups.iter().any(|(_, signals)| signals.is_stopped()) {
            return;
        }
        for sig in [Signal::SIGHUP, Signal::SIGCONT] {
            for (_, signals) in &groups {
                signals.send_kernel(sig.0);
            }
        }
    }

    /// Whether the process group of `process` is orphaned (`is_current_pgrp_orphaned`).
    pub(crate) fn is_pgrp_orphaned(&self, process: &Process) -> bool {
        let pgid = process.ident.lock_or_recover().pgid;
        self.jobs.is_pgrp_orphaned(pgid)
    }

    /// `do_notify_parent_cldstop`: `process` stopped (`CLD_STOPPED`, `status` the signal) or
    /// SIGCONT continued it (`CLD_CONTINUED`, `status` SIGCONT). Its parent gets SIGCHLD unless
    /// it ignores it or set `SA_NOCLDSTOP`, and its waiters look again either way.
    pub(crate) fn notify_parent_cldstop(&self, process: &Process, why: SiCode, status: i32) {
        let (parent, pid) = {
            let tree = self.procs.tree.lock_or_recover();
            let parent = tree.nodes.get(&process.pid.0).map_or(0, |node| node.parent);
            (parent, tree.nr_for(parent, process.pid.0))
        };
        let parent_signals = (parent != 0).then(|| self.group_signals(parent)).flatten();
        if let Some(signals) = parent_signals
            && !signals.nocldstop()
        {
            let uid = process.cred().user.real;
            let exit = Exit {
                code: why,
                status,
                uid,
            };
            // SIGCHLD needs no room in a queue.
            let _ = signals.send_group(child_info(Signal::SIGCHLD.0, pid, exit));
        }
        self.procs.changed.wake(PollMask::empty());
    }

    /// `exit_files`: the process's tasks let go of their descriptor tables, closing every file
    /// no one else holds.
    pub(crate) fn exit_files(&self, process: &Arc<Process>) {
        let tables: Vec<Arc<Mutex<FdTable>>> = {
            let mut state = self.state.lock_or_recover();
            state
                .tasks
                .values_mut()
                .filter(|t| Arc::ptr_eq(&t.process, process))
                .map(|t| {
                    let empty = Arc::new(Mutex::new(FdTable::new(self.nr_open.clone())));
                    std::mem::replace(&mut t.files, empty)
                })
                .collect()
        };
        // The tasks may be gone by now (another one releasing at the same time took the last),
        // so the files close by the process's pid alone.
        for table in tables {
            let Ok(table) = Arc::try_unwrap(table) else {
                continue;
            };
            let mut table = table.into_inner_or_recover();
            for fd in (0..table.end()).map(|fd| Fd(fd as i32)) {
                if let Some(file) = table.remove(fd) {
                    self.closed_by(process.pid, &file);
                }
            }
        }
    }

    // ---- clone ----

    /// `clone`: the exit signal is in the flags' low byte, and `parent_tid` doubles as where
    /// `CLONE_PIDFD` puts the pidfd.
    pub(crate) async fn sys_clone(
        &self,
        caller: &Caller<'_>,
        flags: Ulong,
        newsp: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> SysResult<i32> {
        let flags = u64::from(flags);
        let args = KernelClone {
            flags: CloneFlags(flags & !CloneFlags::CSIGNAL.0),
            exit_signal: flags & CloneFlags::CSIGNAL.0,
            pidfd: parent_tid.addr(),
            parent_tid: parent_tid.addr(),
            child_tid: child_tid.addr(),
            stack: newsp,
            tls,
            set_tid: Vec::new(),
        };
        self.kernel_clone(caller, args).await
    }

    /// `clone3`: `clone` with its arguments in a size-versioned `struct clone_args`
    /// (`copy_clone_args_from_user`, `clone3_args_valid`).
    pub(crate) async fn sys_clone3(
        &self,
        caller: &Caller<'_>,
        uargs: ConstCloneArgsPtr,
        size: Usize,
    ) -> SysResult<i32> {
        let (args, set_tid) = copy_clone_args(caller, uargs, size).await?;
        let flags = args.flags.0;
        let known = CLONE_LEGACY_FLAGS
            | CloneFlags::CLONE_CLEAR_SIGHAND.0
            | CloneFlags::CLONE_INTO_CGROUP.0
            | CloneFlags::CLONE_AUTOREAP.0
            | CloneFlags::CLONE_NNP.0
            | CloneFlags::CLONE_PIDFD_AUTOKILL.0
            | CloneFlags::CLONE_EMPTY_MNTNS.0;
        // `CLONE_DETACHED` and the signal byte are free for reuse in `clone3`.
        let reserved =
            CloneFlags::CLONE_DETACHED.0 | (CloneFlags::CSIGNAL.0 & !CloneFlags::CLONE_NEWTIME.0);
        let clear_sighand = CloneFlags::CLONE_SIGHAND.0 | CloneFlags::CLONE_CLEAR_SIGHAND.0;
        let thread_or_parent = CloneFlags::CLONE_THREAD.0 | CloneFlags::CLONE_PARENT.0;
        if flags & !known != 0
            || flags & reserved != 0
            || flags & clear_sighand == clear_sighand
            || (flags & thread_or_parent != 0 && args.exit_signal != 0)
        {
            return Err(Errno::EINVAL.into());
        }
        // `clone3_stack_valid`: a stack is its lowest address and a size, inside the address
        // space; the child starts at its top.
        let stack = match (args.stack, args.stack_size) {
            (0, 0) => 0,
            (0, _) | (_, 0) => return Err(Errno::EINVAL.into()),
            (stack, size) => stack
                .checked_add(size)
                .and_then(|top| u32::try_from(top).ok())
                .ok_or(Errno::EINVAL)?,
        };
        let (Ok(pidfd), Ok(parent_tid), Ok(child_tid), Ok(tls)) = (
            u32::try_from(args.pidfd),
            u32::try_from(args.parent_tid),
            u32::try_from(args.child_tid),
            u32::try_from(args.tls),
        ) else {
            return Err(Errno::EFAULT.into());
        };
        let args = KernelClone {
            flags: args.flags,
            exit_signal: args.exit_signal,
            pidfd,
            parent_tid,
            child_tid,
            stack,
            tls,
            set_tid,
        };
        self.kernel_clone(caller, args).await
    }

    /// `kernel_clone` and `copy_process`'s checks of the flags, in their order. vfork
    /// (`CLONE_VM | CLONE_VFORK`) works in every build configuration; fork (no `CLONE_VM`)
    /// needs an address space that can be duplicated, and gets `ENOSYS` otherwise, as there is
    /// no fork without an MMU on Linux.
    async fn kernel_clone(&self, caller: &Caller<'_>, mut args: KernelClone) -> SysResult<i32> {
        // An empty mount namespace is a new one.
        if args.flags.contains(CloneFlags::CLONE_EMPTY_MNTNS) {
            args.flags |= CloneFlags::CLONE_NEWNS;
        }
        let flags = args.flags;
        if flags.contains(CloneFlags::CLONE_PIDFD)
            && flags.contains(CloneFlags::CLONE_PARENT_SETTID)
            && args.pidfd == args.parent_tid
        {
            return Err(Errno::EINVAL.into());
        }
        // `valid_signal`.
        if args.exit_signal > 64 {
            return Err(Errno::EINVAL.into());
        }
        let exit_signal = args.exit_signal as i32;
        let both = |a: CloneFlags, b: CloneFlags| flags.contains(a) && flags.contains(b);
        // A child in another mount or user namespace cannot share the root.
        if both(CloneFlags::CLONE_NEWNS, CloneFlags::CLONE_FS)
            || both(CloneFlags::CLONE_NEWUSER, CloneFlags::CLONE_FS)
        {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_THREAD) && !flags.contains(CloneFlags::CLONE_SIGHAND) {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_SIGHAND) && !flags.contains(CloneFlags::CLONE_VM) {
            return Err(Errno::EINVAL.into());
        }
        // A thread stays in its group's pid and user namespaces.
        if flags.contains(CloneFlags::CLONE_THREAD)
            && (flags.intersects(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWPID)
                || self.pid_ns_for_children_differs(caller.task()))
        {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_PIDFD) && flags.contains(CloneFlags::CLONE_DETACHED) {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_AUTOREAP)
            && (flags.intersects(CloneFlags::CLONE_THREAD | CloneFlags::CLONE_PARENT)
                || exit_signal != 0)
        {
            return Err(Errno::EINVAL.into());
        }
        if both(CloneFlags::CLONE_NNP, CloneFlags::CLONE_THREAD) {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_PIDFD_AUTOKILL)
            && (!flags.contains(CloneFlags::CLONE_PIDFD | CloneFlags::CLONE_AUTOREAP)
                || flags.contains(CloneFlags::CLONE_THREAD))
        {
            return Err(Errno::EINVAL.into());
        }
        // `cgroup_css_set_fork`: there is no cgroup filesystem, so no descriptor is a cgroup
        // directory.
        if flags.contains(CloneFlags::CLONE_INTO_CGROUP) {
            return Err(Errno::EBADF.into());
        }
        // Threads start through `thread-spawn`; a process sharing handlers is not supported.
        if flags.contains(CloneFlags::CLONE_THREAD) || flags.contains(CloneFlags::CLONE_SIGHAND) {
            return Err(Errno::EINVAL.into());
        }
        // No child is reaped without its parent.
        if flags.contains(CloneFlags::CLONE_AUTOREAP) {
            return Err(Errno::EINVAL.into());
        }
        let newsp = args.stack;
        let (parent_tid, tls, child_tid) = (args.parent_tid, args.tls, args.child_tid);
        if !flags.contains(CloneFlags::CLONE_VM) {
            let pidfd = match flags.contains(CloneFlags::CLONE_PIDFD) {
                true => Some(self.prepare_pidfd(caller, args.pidfd, false).await?),
                false => None,
            };
            let writes = ForkWrites {
                parent_tid,
                pidfd,
                tls,
                child_tid,
            };
            return self
                .fork(caller, flags, exit_signal, newsp, writes, &args.set_tid)
                .await;
        }
        // Only vfork can run the child: it takes over the caller's instance.
        if !flags.contains(CloneFlags::CLONE_VFORK) || newsp != 0 {
            return Err(Errno::EINVAL.into());
        }
        let pidfd = match flags.contains(CloneFlags::CLONE_PIDFD) {
            true => Some(self.prepare_pidfd(caller, args.pidfd, true).await?),
            false => None,
        };
        let parent_task = caller.task();
        let child = self.copy_process(
            parent_task,
            flags,
            exit_signal,
            tls,
            child_tid,
            &args.set_tid,
        )?;
        if let Some((reservation, _)) = pidfd {
            reservation.install(Arc::new(self.pidfd_file(child.0, OpenFlags::O_RDWR)));
        }
        // Linux ignores faults on both writes: the parent's is the child's pid as the parent
        // sees it, the child's (`schedule_tail`) as the child does, which differ when it starts
        // a pid namespace.
        if flags.contains(CloneFlags::CLONE_PARENT_SETTID) {
            let tid = self.vnr(caller, child.0);
            let _ = write_value(caller.memory(), GuestPtrMut::new(parent_tid), &tid).await;
        }
        if flags.contains(CloneFlags::CLONE_CHILD_SETTID) {
            let tid = self.own_tid(child);
            let _ = write_value(caller.memory(), GuestPtrMut::new(child_tid), &tid).await;
        }
        let pid = self.vnr(caller, child.0);
        {
            let mut tree = self.procs.tree.lock_or_recover();
            tree.vforks.insert(child, parent_task);
            tree.vfork_pids.insert(child, pid);
        }
        let child_signals = self.task_signals(child).group().clone();
        self.task_signals(parent_task)
            .set_vfork_child(Some((child_signals, child)));
        let switch = GuestAction::SwitchTask {
            task: child,
            ret: 0,
        };
        self.queue(parent_task, switch);
        Ok(0)
    }

    /// Whether `task`'s children would start in another pid namespace than its own.
    fn pid_ns_for_children_differs(&self, task: TaskId) -> bool {
        self.task_ref(task, |t| {
            !Arc::ptr_eq(&t.process.pid_ns, &t.ns.pid_for_children)
        })
    }

    /// `pidfd_prepare` and the `put_user` of its number for `CLONE_PIDFD`, before anything is
    /// made, so a fault makes nothing (`EFAULT`): reserves the parent's descriptor (`EMFILE`)
    /// and, with `write`, writes its number at `at`. Without `write` (fork, whose child must
    /// not see the number), only checks that it would land there: the execution layer writes
    /// it after the copy of the address space ([`GuestAction::Fork`]).
    async fn prepare_pidfd(
        &self,
        caller: &Caller<'_>,
        at: u32,
        write: bool,
    ) -> Result<(FdReservation, u32), Errno> {
        let [reservation] = self.reserve_fds::<1>(caller, true)?;
        let fd = reservation.fd().0;
        match write {
            true => write_value(caller.memory(), GuestPtrMut::new(at), &fd)
                .await
                .map_err(|_| Errno::EFAULT)?,
            false => self.check_put_user(caller, at, 4).await?,
        }
        Ok((reservation, at))
    }

    /// Whether a `put_user` of `len` bytes at `addr` would land in the caller's memory, as
    /// [`UserMemory::write`] decides it, without writing: inside the memory, above its low
    /// guard, and where the page map allows writes.
    async fn check_put_user(&self, caller: &Caller<'_>, addr: u32, len: u32) -> Result<(), Errno> {
        let memory = caller.memory();
        let end = u64::from(addr) + u64::from(len);
        if addr < memory.low_guard().await || end > memory.size().await {
            return Err(Errno::EFAULT);
        }
        let access = self.mm(caller.task()).access().clone();
        access
            .check(addr, len as usize, true)
            .map_err(|_| Errno::EFAULT)
    }

    /// The parent's pidfd of the process `pid` (`pidfd_prepare`).
    fn pidfd_file(&self, pid: u32, status: OpenFlags) -> OpenFile {
        let pid_ns = match self.procs.find(pid) {
            Some(process) => process.pid_ns.clone(),
            None => self.init_ns.pid_for_children.clone(),
        };
        let object = PidFd {
            pid,
            pid_ns,
            tree: self.procs.tree.clone(),
            changed: self.procs.changed.clone(),
        };
        OpenFile::new(
            Backing::Stream(Arc::new(object), self.pseudo.anon("[pidfd]")),
            status,
        )
    }

    /// fork: `clone` without `CLONE_VM`. The child gets a copy of the address space, which the
    /// execution layer makes once the call returns ([`GuestAction::Fork`]); clone's writes to
    /// guest memory wait for that copy, so each lands only in the address space Linux writes.
    /// With `CLONE_VFORK`, the parent then waits until the child is done with its copy
    /// ([`Kernel::vfork_wait`]).
    async fn fork(
        &self,
        caller: &Caller<'_>,
        flags: CloneFlags,
        exit_signal: i32,
        newsp: Uptr,
        writes: ForkWrites,
        set_tid: &[i32],
    ) -> SysResult<i32> {
        let ForkWrites {
            parent_tid,
            pidfd,
            tls,
            child_tid,
        } = writes;
        // The child cannot start on another stack (`newsp`).
        if newsp != 0 {
            return Err(Errno::EINVAL.into());
        }
        if !caller.memory().can_duplicate().await {
            return Err(Errno::ENOSYS.into());
        }
        let parent_task = caller.task();
        let child = self.copy_process(parent_task, flags, exit_signal, tls, child_tid, set_tid)?;
        let pidfd = pidfd.map(|(reservation, at)| {
            let file = self.pidfd_file(child.0, OpenFlags::O_RDWR);
            (at, reservation.install(Arc::new(file)).0)
        });
        let set = |flag: CloneFlags, addr: u32| flags.contains(flag).then_some(addr);
        let pid = self.vnr(caller, child.0);
        // `p->vfork_done`: the child's `mm_release` lets the parent go.
        let vfork = flags.contains(CloneFlags::CLONE_VFORK);
        if vfork {
            let mut tree = self.procs.tree.lock_or_recover();
            tree.fork_vforks.insert(child, parent_task);
        }
        let fork = GuestAction::Fork {
            child,
            pid,
            tid: self.own_tid(child),
            parent_tid: set(CloneFlags::CLONE_PARENT_SETTID, parent_tid),
            pidfd,
            child_tid: set(CloneFlags::CLONE_CHILD_SETTID, child_tid),
            tls: set(CloneFlags::CLONE_SETTLS, tls),
            vfork,
        };
        self.queue(parent_task, fork);
        Ok(pid)
    }

    /// `copy_process` for a new thread group: the child gets copies of what `flags` does not
    /// share, the caller's mask, a pid in its pid namespace and each one above (those
    /// `set_tid` asks for, from the innermost), and a place in the tree. Without `CLONE_VM`,
    /// that includes the address space's mappings and break (`dup_mm`). The first process of
    /// a pid namespace is its init, which only signals it handles reach from inside.
    fn copy_process(
        &self,
        parent_task: TaskId,
        flags: CloneFlags,
        exit_signal: i32,
        tls: u32,
        child_tid: u32,
        set_tid: &[i32],
    ) -> Result<TaskId, Errno> {
        let (id, process, parent_pid) = {
            let mut state = self.state.lock_or_recover();
            let task = &state.tasks[&parent_task];
            let parent = task.process.clone();
            // Siblings of an init would not be reaped by their parent, as nothing is above it
            // in its namespace.
            if flags.contains(CloneFlags::CLONE_PARENT) && task.signals.group().unkillable() {
                return Err(Errno::EINVAL);
            }
            if crate::resource::nproc_exceeded(&state, &parent) {
                return Err(Errno::EAGAIN);
            }
            // `fatal_signal_pending`: a group on its way out, or whose `execve` is killing its
            // threads, starts nothing.
            if parent.group.signal_group_exit() {
                return Err(Errno::EINTR);
            }
            let sched = task.sched.for_child()?;
            let files = match flags.contains(CloneFlags::CLONE_FILES) {
                true => task.files.clone(),
                false => Arc::new(Mutex::new(task.files.lock_or_recover().dup()?)),
            };
            // The child's own root and working directory, which a new mount namespace moves
            // (`copy_fs` before `copy_namespaces`); `CLONE_NEWNS` never goes with `CLONE_FS`.
            let mut own_fs =
                (!flags.contains(CloneFlags::CLONE_FS)).then(|| task.fs.lock_or_recover().clone());
            let ns = task.ns.copy(
                self,
                flags.0,
                &parent.cred(),
                &parent.pid_ns,
                own_fs.as_mut(),
            )?;
            let mut ident = parent.ident.lock_or_recover().for_child();
            if flags.contains(CloneFlags::CLONE_NNP) {
                ident.no_new_privs = true;
            }
            let id = TaskId(state.last_id + 1);
            // `checkpoint_restore_ns_capable`.
            let cred = parent.cred();
            let may_set = cred.capable(Cap::CheckpointRestore) || cred.capable(Cap::SysAdmin);
            state
                .pids
                .alloc(id.0, &ns.pid_for_children, set_tid, may_set)?;
            let process = Arc::new(Process {
                pid: id,
                args: Mutex::new(parent.args.lock_or_recover().clone()),
                env: Mutex::new(parent.env.lock_or_recover().clone()),
                exe: Mutex::new(parent.exe.lock_or_recover().clone()),
                group: thread::Group::new(),
                start: self.host.monotonic(),
                // A child's times start at zero, and it inherits no interval timers.
                cpu: Arc::default(),
                children: Mutex::default(),
                maxrss: Default::default(),
                cpu_timers: Default::default(),
                ident: Mutex::new(ident),
                cred: Mutex::new(parent.cred()),
                resources: Mutex::new(parent.resources.lock_or_recover().clone()),
                pid_ns: ns.pid_for_children.clone(),
                sysvsem: Mutex::new(ns.ipc.clone()),
            });
            let fs = match own_fs {
                None => task.fs.clone(),
                Some(fs) => Arc::new(Mutex::new(fs)),
            };
            let flag = Arc::new(WorkFlag::default());
            let account = crate::signal::Account::new(self.sigqueue_users.clone(), &process);
            let group = task.signals.group().fork(account);
            if flags.contains(CloneFlags::CLONE_CLEAR_SIGHAND) {
                group.flush_handlers();
            }
            // `is_child_reaper`.
            if process.pid_ns.reaper() == Some(id.0) && process.pid_ns.level() > 0 {
                group.make_unkillable();
            }
            let signals = group.attach(id, task.signals.inherit(flags), flag.clone());
            // A vfork child runs on its parent's memory, so it shares the address space; a fork
            // child gets a copy of its mappings and break over the memory the execution layer
            // copies.
            let mm = match flags.contains(CloneFlags::CLONE_VM) {
                true => task.mm.clone(),
                false => Arc::new(task.mm.dup(id.0)),
            };
            let new = Task {
                cpu: Arc::new(CpuAccount::new(process.cpu.clone())),
                process: process.clone(),
                mm,
                files,
                fs,
                ns,
                work: Work::new(flag),
                restart: None,
                comm: task.comm,
                sched,
                thread: task.thread.for_process(flags, tls, child_tid),
                signals,
            };
            state.last_id += 1;
            state.tasks.insert(id, new);
            (id, process, parent.pid.0)
        };
        let parent = match flags.contains(CloneFlags::CLONE_PARENT) {
            true => {
                let tree = self.procs.tree.lock_or_recover();
                tree.nodes.get(&parent_pid).map_or(0, |node| node.parent)
            }
            false => parent_pid,
        };
        // It inherits `RLIMIT_CPU`, and with it the expiry it sets (`posix_cputimers_group_init`).
        process.rlimit_cpu_changed();
        self.procs.add(process, parent, exit_signal);
        Ok(id)
    }

    // ---- execve ----

    /// `do_execveat_common`: loads the file, then, past the point of no return, replaces the
    /// image. The execution layer starts the new image on the way back to the guest.
    pub(crate) async fn sys_execveat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        filename: Cstr,
        argv: CstrArray,
        envp: CstrArray,
        flags: AtFlags,
    ) -> SysResult<i32> {
        let known = AtFlags::AT_EMPTY_PATH | AtFlags::AT_SYMLINK_NOFOLLOW;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let path = crate::fs::read_path(caller.memory(), filename).await?;
        let args = read_strings(caller, argv).await?;
        let env = read_strings(caller, envp).await?;
        self.do_execveat(caller, dfd, path, args, env, flags).await
    }

    /// `kernel_execve`, as Linux starts init: makes `task`, a process [`Kernel::spawn`] started
    /// that runs no image yet, execute the file at `path`, resolved from its working
    /// directory, with `args` and `env` replacing those it was spawned with. Everything
    /// `execve` checks and does applies, scripts included. Returns the image to run as the
    /// process, which the execution layer would otherwise take on the way back to the guest;
    /// on failure the process is unchanged.
    pub async fn exec(
        &self,
        task: TaskId,
        path: &[u8],
        args: Vec<Vec<u8>>,
        env: Vec<Vec<u8>>,
    ) -> Result<Image, Errno> {
        let memory = NoMemory;
        let caller = Caller::new(task, &memory);
        let cwd = Fd(wasixv2_abi::AtFd::AT_FDCWD.0);
        self.do_execveat(&caller, cwd, path.to_vec(), args, env, AtFlags(0))
            .await
            .map_err(|err| match err {
                SysErr::Errno(errno) => errno,
                // Nothing waits here for a signal to interrupt.
                _ => Errno::EINTR,
            })?;
        // There is no guest to return to: the image goes to the caller, not through the work
        // queue. Only the execve's own action goes; whatever else is queued, such as an exit
        // the embedder asked for before the image started, stays, and so does the work flag.
        self.with_task(task, |t| t.work.take_exec())
            .ok_or(Errno::EINTR)
    }

    /// `do_execveat_common` once the arguments are in the kernel.
    async fn do_execveat(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: Vec<u8>,
        mut args: Vec<Vec<u8>>,
        env: Vec<Vec<u8>>,
        flags: AtFlags,
    ) -> SysResult<i32> {
        // Linux 5.18+: an empty argv gets an empty argv[0], so argc is never 0.
        if args.is_empty() {
            args.push(Vec::new());
        }
        check_arg_size(&args, &env)?;
        // The name the binary is known by: what the caller passed, or `/dev/fd/N` for an fd.
        let fd_relative = !path.starts_with(b"/") && dfd.0 != wasixv2_abi::AtFd::AT_FDCWD.0;
        let name = match (path.is_empty(), fd_relative) {
            (true, _) => format!("/dev/fd/{}", dfd.0).into_bytes(),
            (false, false) => path.clone(),
            (false, true) => format!("/dev/fd/{}/", dfd.0)
                .into_bytes()
                .into_iter()
                .chain(path.iter().copied())
                .collect(),
        };
        // `BINPRM_FLAGS_PATH_INACCESSIBLE`: a script named through a close-on-exec descriptor
        // could not be opened by its interpreter.
        let inaccessible = (path.is_empty() || fd_relative)
            && self.files(caller.task()).lock_or_recover().cloexec(dfd) == Some(true);
        let (mut file, mut binary) = self.open_exec(caller, dfd, &path, flags).await?;
        // `bprm->interp`: the name a script is passed to its interpreter by.
        let mut script = name.clone();
        let mut rewrites = 0;
        // `search_binary_handler`: a script hands over to its interpreter; anything else must
        // be an image of one of the embedder's formats, decided before the point of no return.
        let (image, binary) = loop {
            // `prepare_binprm`: the first bytes tell the format.
            let header = read_header(&file).await?;
            // `load_script`.
            let Some((interp, arg)) = parse_shebang(&header) else {
                self.binfmt.probe(&header)?;
                let image = read_image(&file, binary.size).await?;
                self.binfmt.check(&image)?;
                break (image, binary);
            };
            if inaccessible {
                return Err(Errno::ENOENT.into());
            }
            // argv[0] goes; the interpreter, its argument and the script's name come first.
            let mut front = vec![interp.clone()];
            front.extend(arg);
            front.push(script);
            args.splice(0..1, front);
            check_arg_size(&args, &env)?;
            let cwd = Fd(wasixv2_abi::AtFd::AT_FDCWD.0);
            (file, binary) = self.open_exec(caller, cwd, &interp, AtFlags(0)).await?;
            script = interp;
            rewrites += 1;
            if rewrites > MAX_REWRITES {
                return Err(Errno::ELOOP.into());
            }
        };
        // `de_thread`: the other threads die before anything of the new image is committed. A
        // group on its way out does not exec; the exit it is taking ends the caller on the way
        // back.
        let task = caller.task();
        let exec = self.zap_threads(task)?;
        let leader = self.process(task).pid;
        // The leader leaves its old image for the new one, which runs once committed.
        let image = match task == leader {
            true => Some(image),
            false => {
                self.hand_image(leader, image);
                None
            }
        };
        self.de_thread(task).await?;
        // `exec_mm_release` for the caller, and for the leader when that is another task, on
        // the memory that is still theirs; the threads `de_thread` ended left it as each went.
        // Then the old memory goes.
        let threads = self.take_group_futex_states(task);
        self.exit_futexes(caller, threads).await;
        self.mm_release(caller).await;
        self.exec_commit(task, &name, args, env, image, binary);
        // The `execve` is done, and the leader's new image may start.
        drop(exec);
        Ok(0)
    }

    /// `do_open_execat`: the file must be a regular file with an execute bit the caller may
    /// use, on a mount that allows execution. Returns it open for reading, and the binary.
    async fn open_exec(
        &self,
        caller: &Caller<'_>,
        dfd: Fd,
        path: &[u8],
        flags: AtFlags,
    ) -> SysResult<(wasixv2_vfs::File, Binary)> {
        if path.is_empty() && !flags.contains(AtFlags::AT_EMPTY_PATH) {
            return Err(Errno::ENOENT.into());
        }
        let env = self.env(caller);
        let target = self
            .resolve_at(caller, &env, dfd, path, flags, true)
            .await?;
        let attr = self.vfs.stat(&target).await?;
        match attr.file_type {
            FileType::Regular => {}
            FileType::Symlink => return Err(Errno::ELOOP.into()),
            _ => return Err(Errno::EACCES.into()),
        }
        if target.mnt.flags().noexec {
            return Err(Errno::EACCES.into());
        }
        self.vfs.permission(&env.cred, &target, MAY_EXEC).await?;
        let how = OpenHow {
            flags: OpenFlags::O_RDONLY,
            mode: 0,
            lookup: LookupFlags {
                empty: true,
                ..LookupFlags::default()
            },
        };
        let file = self.vfs.open(&env.cx(), &target, b"", &how).await?;
        let binary = Binary {
            mode: attr.mode,
            owner: (attr.uid, attr.gid),
            nosuid: target.mnt.flags().nosuid,
            size: attr.size,
            path: target,
        };
        Ok((file, binary))
    }

    /// Past the point of no return (`begin_new_exec`), once the other threads are gone: the
    /// image runs as the group leader (the end of `de_thread`), close-on-exec descriptors close,
    /// caught signals reset, the new image's arguments, environment and memory replace the old
    /// ones, and the credentials become what `binary` grants. `image` goes to the leader now,
    /// unless it has it already; a vfork parent resumes once the image starts.
    fn exec_commit(
        &self,
        caller: TaskId,
        name: &[u8],
        args: Vec<Vec<u8>>,
        env: Vec<Vec<u8>>,
        image: Option<Image>,
        binary: Binary,
    ) {
        let task = self.exec_leader(caller);
        let process = self.process(task);
        {
            let mut tree = self.procs.tree.lock_or_recover();
            if let Some(node) = tree.nodes.get_mut(&process.pid.0) {
                node.did_exec = true;
            }
        }
        // `unshare_files`, then `do_close_on_exec`. The dying threads of the group do not
        // count as sharing the table.
        let table = {
            let state = self.state.lock_or_recover();
            let files = state.tasks[&task].files.clone();
            let shared = state
                .tasks
                .values()
                .any(|t| !Arc::ptr_eq(&t.process, &process) && Arc::ptr_eq(&t.files, &files));
            (files, shared)
        };
        let table = match table {
            (files, false) => files,
            (files, true) => {
                let copy = Arc::new(Mutex::new(files.lock_or_recover().copy()));
                self.with_task(task, |t| t.files = copy.clone());
                copy
            }
        };
        let closing: Vec<Arc<OpenFile>> = {
            let mut table = table.lock_or_recover();
            let cloexec: Vec<Fd> = (0..table.end())
                .map(|fd| Fd(fd as i32))
                .filter(|&fd| table.cloexec(fd) == Some(true))
                .collect();
            cloexec
                .into_iter()
                .filter_map(|fd| table.remove(fd))
                .collect()
        };
        let memory = NoMemory;
        let caller = Caller::new(task, &memory);
        for file in closing {
            self.closed(&caller, &file);
        }
        // `exit_itimers` and `flush_itimer_signals`: POSIX timers belong to the old image;
        // interval timers stay.
        let signals = self.task_signals(task);
        crate::posix_timer::exit_itimers(signals.group());
        signals.group().flush_timer_signals();
        signals.group().exec_reset();
        *process.args.lock_or_recover() = args;
        *process.env.lock_or_recover() = env;
        let no_new_privs = process.ident.lock_or_recover().no_new_privs;
        let old = process.cred();
        let (cred, secure) = old.exec(
            binary.mode,
            binary.owner,
            !binary.nosuid && !no_new_privs,
            no_new_privs,
        );
        let dumpable =
            cred.user.effective == cred.user.real && cred.group.effective == cred.group.real;
        let privileged = secure || cred.drops_dumpable(&old);
        *process.cred.lock_or_recover() = cred;
        *process.exe.lock_or_recover() = Some(binary.path);
        {
            let mut ident = process.ident.lock_or_recover();
            ident.secure_exec = secure;
            ident.dumpable = dumpable;
            if privileged {
                ident.clear_pdeathsig();
            }
        }
        self.with_task(task, |t| {
            t.comm = Comm::from_path(name);
            // `exec_mmap`: the new image gets its own address space. A vfork child leaves its
            // parent's untouched.
            t.mm = Arc::default();
            t.thread = thread::ThreadState::default();
        });
        // `exec_mm_release`: a parent that waits for the old memory goes on.
        self.complete_vfork_done(task);
        if let Some(image) = image {
            self.hand_image(task, image);
        }
    }

    // ---- wait ----

    /// `kernel_waitid`.
    pub(crate) async fn sys_waitid(
        &self,
        caller: &Caller<'_>,
        which: Idtype,
        upid: Pid,
        infop: SiginfoPtr,
        options: WaitOptions,
        ru: RusagePtr,
    ) -> SysResult<i32> {
        let options = options.0;
        if options & !WAIT_KNOWN != 0 {
            return Err(Errno::EINVAL.into());
        }
        let wanted = WaitOptions::WEXITED.0 | WaitOptions::WSTOPPED.0 | WaitOptions::WCONTINUED.0;
        if options & wanted == 0 {
            return Err(Errno::EINVAL.into());
        }
        let me = self.process(caller.task());
        // `find_get_pid`: a pid outside the caller's namespace names no child (id 0 is none).
        let find = |nr: Pid| self.find_vpid(caller, nr).unwrap_or(0);
        let which = match which {
            Idtype::P_ALL => Which::All,
            Idtype::P_PID if upid > 0 => Which::Pid(find(upid)),
            Idtype::P_PGID if upid == 0 => Which::Pgid(me.ident.lock_or_recover().pgid),
            Idtype::P_PGID if upid > 0 => Which::Pgid(find(upid)),
            Idtype::P_PIDFD => Which::Pid(self.pidfd_pid(caller, Fd(upid))?),
            _ => return Err(Errno::EINVAL.into()),
        };
        let interrupt = self.interrupt(caller);
        let found = wait_event(
            &self.procs.changed,
            || self.wait_once(&me, which, options),
            &*interrupt,
        )
        .await
        .map_err(|_| SysErr::Restart)??;
        // A child the wait reaped may leave a pid namespace's dying init nothing to wait for.
        self.pid_ns_progress();
        let (info, usage) = match found {
            Some((pid, exit, child)) => {
                let usage = child.usage_both();
                (child_info(Signal::SIGCHLD.0, pid, exit), usage)
            }
            None => (
                Siginfo {
                    si_signo: 0,
                    si_errno: 0,
                    si_code: 0,
                    sifields: [0; 116],
                },
                Rusage::default(),
            ),
        };
        if !ru.is_null() {
            write_value(caller.memory(), ru, &usage).await?;
        }
        if !infop.is_null() {
            write_value(caller.memory(), infop, &info).await?;
        }
        Ok(0)
    }

    /// One pass of `do_wait`: a child to report, `Ok(None)` for `WNOHANG` when none is,
    /// `ECHILD` without matching children, and `None` to keep waiting. A zombie comes first
    /// (`wait_task_zombie`); then, for `WSTOPPED`, a stopped child whose stop no `waitid`
    /// reported yet (`wait_task_stopped`), and for `WCONTINUED` a continued one
    /// (`wait_task_continued`). `WNOWAIT` leaves what it reports to be reported again.
    /// A reaped zombie's time and its reaped children's go to `me`'s `cutime` and `cstime`.
    /// The child's pid is as `me` sees it (`task_pid_vnr`).
    #[allow(clippy::type_complexity)]
    fn wait_once(
        &self,
        me: &Process,
        which: Which,
        options: u32,
    ) -> Option<Result<Option<(Pid, Exit, Arc<Process>)>, Errno>> {
        let mut tree = self.procs.tree.lock_or_recover();
        let mut any = false;
        let mut ready = None;
        let mut live = Vec::new();
        for (&pid, node) in tree.children(me.pid.0) {
            let matches = match which {
                Which::All => true,
                Which::Pid(want) => pid == want,
                Which::Pgid(pgid) => node.process.ident.lock_or_recover().pgid == pgid,
            };
            // `eligible_child`: `__WCLONE` picks the children that report with another
            // signal than SIGCHLD; `__WALL` takes both.
            let clone_child = node.exit_signal != Signal::SIGCHLD.0;
            let kind =
                options & WaitOptions::__WALL.0 != 0 || clone_child == (options & WCLONE != 0);
            if !matches || !kind {
                continue;
            }
            any = true;
            match node.exit {
                Some(exit) if options & WaitOptions::WEXITED.0 != 0 => {
                    ready = Some((pid, exit, node.process.clone()));
                    break;
                }
                Some(_) => {}
                None => live.push((pid, node.process.clone())),
            }
        }
        // The pid as `me` sees it, before reaping frees it.
        let mut ready =
            ready.map(|(pid, exit, child)| (pid, tree.pids.nr(&me.pid_ns, pid), exit, child));
        if let Some((pid, _, _, child)) = &ready
            && options & WaitOptions::WNOWAIT.0 == 0
        {
            tree.reap(*pid);
            me.add_child_usage(child);
        }
        drop(tree);
        if ready.is_none() {
            // The group's signal state comes after the tree, with the tree unlocked.
            live.sort_by_key(|(pid, _)| *pid);
            ready = live.into_iter().find_map(|(pid, process)| {
                let nr = self.pids.nr(&me.pid_ns, pid);
                Some((pid, nr, self.stop_event(&process, options)?, process))
            });
        }
        let ready = ready.map(|(_, nr, exit, child)| (nr, exit, child));
        match (ready, any) {
            (Some(found), _) => Some(Ok(Some(found))),
            (None, false) => Some(Err(Errno::ECHILD)),
            (None, true) if options & WaitOptions::WNOHANG.0 != 0 => Some(Ok(None)),
            (None, true) => None,
        }
    }

    /// The stop or continue of the live child `process` a `waitid` with `options` reports.
    fn stop_event(&self, process: &Process, options: u32) -> Option<Exit> {
        let wanted = WaitOptions::WSTOPPED.0 | WaitOptions::WCONTINUED.0;
        if options & wanted == 0 {
            return None;
        }
        let signals = self.group_signals(process.pid.0)?;
        let nowait = options & WaitOptions::WNOWAIT.0 != 0;
        let uid = process.cred().user.real;
        if options & WaitOptions::WSTOPPED.0 != 0
            && let Some(sig) = signals.wait_stopped(nowait)
        {
            return Some(Exit {
                code: SiCode::CLD_STOPPED,
                status: sig,
                uid,
            });
        }
        let continued = Exit {
            code: SiCode::CLD_CONTINUED,
            status: Signal::SIGCONT.0,
            uid,
        };
        (options & WaitOptions::WCONTINUED.0 != 0 && signals.wait_continued(nowait))
            .then_some(continued)
    }

    // ---- process groups and sessions ----

    /// The process group of the process `pid`, by ids.
    pub(crate) fn pgid_of(&self, pid: u32) -> u32 {
        match self.procs.find(pid) {
            Some(process) => process.ident.lock_or_recover().pgid,
            None => pid,
        }
    }

    /// The thread groups in process group `pgid` that have not exited.
    pub(crate) fn pgrp_groups(&self, pgid: u32) -> Vec<(u32, Arc<Signals>)> {
        self.jobs.pgrp_groups(pgid)
    }

    /// `setpgid`: the caller or one of its children that has not run `execve`, in the caller's
    /// session, joins or creates a group there.
    pub(crate) fn sys_setpgid(&self, caller: &Caller<'_>, pid: Pid, pgid: Pid) -> SysResult<i32> {
        if pgid < 0 {
            return Err(Errno::EINVAL.into());
        }
        let me = self.process(caller.task());
        let my_sid = me.ident.lock_or_recover().sid;
        let (pid, pgid) = match (pid, pgid) {
            (0, 0) => (me.pid.0, me.pid.0),
            (0, pgid) => (me.pid.0, self.find_vpid(caller, pgid).unwrap_or(0)),
            (pid, pgid) => {
                // `find_task_by_vpid`.
                let pid = self.find_vpid(caller, pid).ok_or(Errno::ESRCH)?;
                match pgid {
                    0 => (pid, pid),
                    pgid => (pid, self.find_vpid(caller, pgid).unwrap_or(0)),
                }
            }
        };
        let tree = self.procs.tree.lock_or_recover();
        let node = tree.nodes.get(&pid).ok_or(Errno::ESRCH)?;
        if pid != me.pid.0 {
            if node.parent != me.pid.0 || node.exit.is_some() {
                return Err(Errno::ESRCH.into());
            }
            if node.process.ident.lock_or_recover().sid != my_sid {
                return Err(Errno::EPERM.into());
            }
            if node.did_exec {
                return Err(Errno::EACCES.into());
            }
        }
        let mut ident = node.process.ident.lock_or_recover();
        if ident.sid == pid {
            return Err(Errno::EPERM.into());
        }
        // A group named by a pid outside the caller's namespace does not exist for it.
        if pgid != pid {
            let exists = pgid != 0
                && tree.nodes.iter().any(|(&other, n)| {
                    n.exit.is_none()
                        && match other == pid {
                            true => ident.pgid == pgid,
                            false => {
                                let id = n.process.ident.lock_or_recover();
                                id.pgid == pgid && id.sid == my_sid
                            }
                        }
                });
            if !exists {
                return Err(Errno::EPERM.into());
            }
        }
        tree.set_pgrp(&mut ident, pgid, None);
        Ok(0)
    }

    /// `setsid`: fails for a group leader, or when a group with the caller's pid exists.
    pub(crate) fn sys_setsid(&self, caller: &Caller<'_>) -> SysResult<Pid> {
        let process = self.process(caller.task());
        let me = process.pid.0;
        let tree = self.procs.tree.lock_or_recover();
        let taken = tree.nodes.iter().any(|(&pid, node)| {
            pid != me && node.exit.is_none() && node.process.ident.lock_or_recover().pgid == me
        });
        let mut ident = process.ident.lock_or_recover();
        if ident.pgid == me || taken {
            return Err(Errno::EPERM.into());
        }
        tree.set_pgrp(&mut ident, me, Some(me));
        // `proc_clear_tty`: a new session starts without a controlling terminal.
        ident.tty = None;
        drop(ident);
        drop(tree);
        Ok(self.vnr(caller, me))
    }

    // ---- pidfd ----

    pub(crate) fn sys_pidfd_open(
        &self,
        caller: &Caller<'_>,
        pid: Pid,
        flags: u32,
    ) -> SysResult<Fd> {
        if flags & !PIDFD_NONBLOCK != 0 || pid <= 0 {
            return Err(Errno::EINVAL.into());
        }
        // `find_get_pid`, in the caller's pid namespace.
        let pid = self.find_vpid(caller, pid).ok_or(Errno::ESRCH)?;
        {
            let tree = self.procs.tree.lock_or_recover();
            if !tree.nodes.contains_key(&pid) {
                return Err(Errno::ESRCH.into());
            }
        }
        let mut status = OpenFlags::O_RDWR;
        if flags & PIDFD_NONBLOCK != 0 {
            status |= OpenFlags::O_NONBLOCK;
        }
        let file = self.pidfd_file(pid, status);
        let [fd] = self.install_files(caller, [file], true)?;
        Ok(fd)
    }

    /// `pidfd_get_pid`: the process of the pidfd `fd`; `EBADF` for any other file.
    fn pidfd_pid(&self, caller: &Caller<'_>, fd: Fd) -> Result<u32, Errno> {
        let file = self.io_file(caller, fd)?;
        pidfd_target(&file).ok_or(Errno::EBADF)
    }

    pub(crate) async fn sys_pidfd_send_signal(
        &self,
        caller: &Caller<'_>,
        pidfd: Fd,
        sig: Signal,
        info: ConstSiginfoPtr,
        flags: u32,
    ) -> SysResult<i32> {
        if flags != 0 {
            return Err(Errno::EINVAL.into());
        }
        // `pidfd_to_pid`: a `/proc/<pid>` directory stands for its process here, and only here.
        let file = self.io_file(caller, pidfd)?;
        let pid = pidfd_target(&file)
            .or_else(|| crate::procfs::tgid_dir(file.path()))
            .ok_or(Errno::EBADF)?;
        // `access_pidfd_pidns`: only a process in the caller's pid namespace or below.
        let target_ns = match file.downcast::<PidFd>() {
            Some(pidfd) => Some(pidfd.pid_ns.clone()),
            None => self.procs.find(pid).map(|process| process.pid_ns.clone()),
        };
        let active = self.active_pid_ns(caller.task());
        if target_ns.is_some_and(|ns| !crate::ns::pid_ns_descends(&ns, &active)) {
            return Err(Errno::EINVAL.into());
        }
        // `kill_pid_info_type`: a zombie takes the signal and does nothing with it; a reaped
        // process is `ESRCH`.
        let info = match info.is_null() {
            true => self.sender_info(caller, sig.0, SiCode::SI_USER),
            false => {
                let info = read_value(caller.memory(), info).await?;
                if info.si_signo != sig.0 {
                    return Err(Errno::EINVAL.into());
                }
                // Only the caller itself may be sent a `siginfo` that claims to be the kernel's.
                if caller.task().0 != pid && crate::signal::reserved_to_kernel(info.si_code) {
                    return Err(Errno::EPERM.into());
                }
                info
            }
        };
        let sender = self.process(caller.task());
        self.send_to_group(Some(&sender), pid, info)?;
        Ok(0)
    }
}

/// The process a pidfd refers to (`pidfd_pid`), if `file` is one.
pub(crate) fn pidfd_target(file: &OpenFile) -> Option<u32> {
    file.downcast::<PidFd>().map(|pidfd| pidfd.pid)
}

/// A process descriptor (`pidfd_open`): readable once the process exited.
struct PidFd {
    pid: u32,
    /// The process's pid namespace (`ns_of_pid`), which outlives its pids.
    pid_ns: Arc<PidNs>,
    tree: Arc<Mutex<Tree>>,
    pub(crate) changed: Arc<WaitQueue>,
}

impl Pollable for PidFd {
    /// `pidfd_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.changed);
        let tree = self.tree.lock_or_recover();
        match tree.nodes.get(&self.pid) {
            Some(node) if node.exit.is_none() => PollMask::empty(),
            _ => PollMask::READABLE | PollMask::HUP,
        }
    }
}

impl FileObject for PidFd {
    fn read<'a>(&'a self, _: Io<'a>, _: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

/// clone's writes that a fork leaves to the execution layer, and the pidfd's reservation and
/// where its number goes.
struct ForkWrites {
    parent_tid: u32,
    pidfd: Option<(FdReservation, u32)>,
    tls: u32,
    child_tid: u32,
}

/// The flags `clone` takes (`CLONE_LEGACY_FLAGS`).
const CLONE_LEGACY_FLAGS: u64 = 0xffff_ffff;

/// What `clone` and `clone3` ask for (`struct kernel_clone_args`), as guest addresses.
struct KernelClone {
    /// Without the exit signal.
    flags: CloneFlags,
    exit_signal: u64,
    pidfd: u32,
    parent_tid: u32,
    child_tid: u32,
    /// The top of the child's stack, 0 for the caller's.
    stack: u32,
    tls: u32,
    /// The pids asked for, from the child's pid namespace up (`set_tid`).
    set_tid: Vec<i32>,
}

/// `copy_clone_args_from_user`: a `struct clone_args` of `size` bytes, and the pids its
/// `set_tid` asks for. Sizes below the first version are `EINVAL`; bytes past the version the
/// kernel knows must be zero (`E2BIG`). Everything is read before `clone3_args_valid` looks at
/// the flags, so a fault there comes first.
async fn copy_clone_args(
    caller: &Caller<'_>,
    uargs: ConstCloneArgsPtr,
    size: Usize,
) -> SysResult<(CloneArgs, Vec<i32>)> {
    const VER0: u32 = CloneArgsSize::CLONE_ARGS_SIZE_VER0.0;
    const VER2: u32 = CloneArgsSize::CLONE_ARGS_SIZE_VER2.0;
    if size > crate::mm::PAGE_SIZE as u32 {
        return Err(Errno::E2BIG.into());
    }
    if size < VER0 {
        return Err(Errno::EINVAL.into());
    }
    let memory = caller.memory();
    let mut bytes = vec![0; size.max(VER2) as usize];
    memory
        .read(uargs.addr(), &mut bytes[..size as usize])
        .await?;
    if bytes[VER2 as usize..].iter().any(|&b| b != 0) {
        return Err(Errno::E2BIG.into());
    }
    let args = CloneArgs::read_from(&bytes[..VER2 as usize]).ok_or(Errno::EFAULT)?;
    if args.set_tid_size > crate::pid::MAX_PID_NS_LEVEL as u64
        || (args.set_tid == 0) != (args.set_tid_size == 0)
        || args.exit_signal & !CloneFlags::CSIGNAL.0 != 0
        || (args.flags.contains(CloneFlags::CLONE_INTO_CGROUP)
            && (args.cgroup > i32::MAX as u64 || size < VER2))
    {
        return Err(Errno::EINVAL.into());
    }
    let set_tid = match args.set_tid {
        0 => Vec::new(),
        addr => {
            let addr = u32::try_from(addr).map_err(|_| Errno::EFAULT)?;
            let mut raw = vec![0; args.set_tid_size as usize * 4];
            memory.read(addr, &mut raw).await?;
            raw.chunks(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        }
    };
    Ok((args, set_tid))
}

/// Reads a NULL-terminated array of string pointers (`copy_strings`). A NULL array is empty.
async fn read_strings(caller: &Caller<'_>, array: CstrArray) -> SysResult<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    if array.is_null() {
        return Ok(out);
    }
    let mut total = 0;
    loop {
        let at = GuestPtr::<Cstr>::new(array.addr() + 4 * out.len() as u32);
        let ptr = read_value(caller.memory(), at).await?;
        if ptr.is_null() {
            return Ok(out);
        }
        if out.len() >= MAX_ARG_STRINGS {
            return Err(Errno::E2BIG.into());
        }
        let bytes = read_cstr(caller.memory(), ptr, MAX_ARG_STRLEN)
            .await
            .map_err(|err| match err {
                CstrError::Fault => Errno::EFAULT,
                CstrError::TooLong => Errno::E2BIG,
            })?;
        total += bytes.len() + 1;
        if total > ARG_MAX {
            return Err(Errno::E2BIG.into());
        }
        out.push(bytes);
    }
}

/// `prepare_binprm`'s read of the first `BINPRM_BUF_SIZE` bytes of `file`, fewer when it is
/// shorter.
async fn read_header(file: &wasixv2_vfs::File) -> SysResult<Vec<u8>> {
    let mut header = vec![0; BINPRM_BUF_SIZE];
    let mut at = 0;
    while at < header.len() {
        match file.read_at(&mut header[at..], at as u64).await? {
            0 => break,
            n => at += n,
        }
    }
    header.truncate(at);
    Ok(header)
}

/// The whole of `file`, which was `size` bytes when opened: the contents its filesystem
/// shares, or else a copy. `ENOMEM` when there is no room for the copy.
async fn read_image(file: &wasixv2_vfs::File, size: u64) -> SysResult<Image> {
    if let Some(contents) = file.shared_contents().await {
        return Ok(Image::shared(contents));
    }
    let size = usize::try_from(size).map_err(|_| Errno::ENOMEM)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Errno::ENOMEM)?;
    bytes.resize(size, 0);
    let mut at = 0;
    while at < bytes.len() {
        match file.read_at(&mut bytes[at..], at as u64).await? {
            0 => break,
            n => at += n,
        }
    }
    bytes.truncate(at);
    Ok(Image::copied(bytes))
}

/// `load_script`'s parse of `#!interpreter [argument]` in the first `BINPRM_BUF_SIZE` bytes:
/// the interpreter is the first word, the argument the rest of the line with surrounding spaces
/// and tabs trimmed, and a NUL ends either. Only spaces and tabs separate (a `\r` stays part of
/// the line). `None`, Linux's `ENOEXEC`, for a file that is no script, a line without an
/// interpreter, or an interpreter path the buffer may have truncated: no newline in the buffer
/// and nothing after the path that ends it.
#[allow(clippy::type_complexity)]
fn parse_shebang(bytes: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    // `bprm->buf`, NUL-padded past the end of a shorter file.
    let mut buf = [0; BINPRM_BUF_SIZE];
    let len = bytes.len().min(BINPRM_BUF_SIZE);
    buf[..len].copy_from_slice(&bytes[..len]);
    if !buf.starts_with(b"#!") {
        return None;
    }
    let spacetab = |b: u8| b == b' ' || b == b'\t';
    // The first index in `first..=last` that is not a space or tab, or that ends a word.
    let next_non_spacetab = |first: usize, last: usize| (first..=last).find(|&i| !spacetab(buf[i]));
    let next_terminator =
        |first: usize, last: usize| (first..=last).find(|&i| spacetab(buf[i]) || buf[i] == 0);
    let buf_end = BINPRM_BUF_SIZE - 1;
    let mut end = match buf.iter().position(|&b| b == b'\n') {
        Some(newline) => newline,
        None => {
            let name = next_non_spacetab(2, buf_end)?;
            next_terminator(name, buf_end)?;
            buf_end
        }
    };
    while spacetab(buf[end - 1]) {
        end -= 1;
    }
    let name = next_non_spacetab(2, end).filter(|&name| name != end)?;
    let sep = next_terminator(name, end);
    let arg = sep
        .filter(|&sep| buf[sep] != 0)
        .and_then(|sep| next_non_spacetab(sep, end));
    // The kernel ends both strings with a NUL at `sep` and `end`; an earlier one ends them first.
    let string = |from: usize, to: usize| {
        let s = &buf[from..to];
        s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())].to_vec()
    };
    Some((
        string(name, sep.unwrap_or(end)),
        arg.map(|arg| string(arg, end)),
    ))
}

/// `E2BIG` when the strings of a new image do not fit `ARG_MAX`.
fn check_arg_size(args: &[Vec<u8>], env: &[Vec<u8>]) -> SysResult<()> {
    let total: usize = args.iter().chain(env).map(|s| s.len() + 1 + 4).sum();
    if total > ARG_MAX {
        return Err(Errno::E2BIG.into());
    }
    Ok(())
}

/// Memory for kernel work outside any syscall, where no guest pointer is followed.
pub(crate) struct NoMemory;

impl wasixv2_syscalls::UserMemory for NoMemory {
    fn read<'a>(
        &'a self,
        _: u32,
        _: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), wasixv2_syscalls::Fault>> {
        Box::pin(async { Err(wasixv2_syscalls::Fault) })
    }

    fn write<'a>(
        &'a self,
        _: u32,
        _: &'a [u8],
    ) -> LocalFuture<'a, Result<(), wasixv2_syscalls::Fault>> {
        Box::pin(async { Err(wasixv2_syscalls::Fault) })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        Box::pin(async { 0 })
    }

    fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
        Box::pin(async { None })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_shebang;

    #[test]
    fn shebang_lines() {
        let parse = |line: &[u8]| parse_shebang(line);
        let interp = |name: &[u8]| Some((name.to_vec(), None));
        let with_arg = |name: &[u8], arg: &[u8]| Some((name.to_vec(), Some(arg.to_vec())));
        assert_eq!(parse(b"#!/bin/sh\necho"), interp(b"/bin/sh"));
        assert_eq!(parse(b"#!/bin/sh"), interp(b"/bin/sh"));
        assert_eq!(
            parse(b"#! /usr/bin/env  python3 -u \nx"),
            with_arg(b"/usr/bin/env", b"python3 -u")
        );
        assert_eq!(parse(b"#!\t/bin/sh\t-e\t\n"), with_arg(b"/bin/sh", b"-e"));
        // Only spaces and tabs separate: a CRLF script names `sh\r`.
        assert_eq!(parse(b"#!/bin/sh\r\n"), interp(b"/bin/sh\r"));
        assert_eq!(parse(b"#!/bin/sh -e\r\n"), with_arg(b"/bin/sh", b"-e\r"));
        // A NUL ends the interpreter and the argument.
        assert_eq!(parse(b"#!/bin/sh\0 -e\n"), interp(b"/bin/sh"));
        assert_eq!(parse(b"#!/bin/sh -e\0x\n"), with_arg(b"/bin/sh", b"-e"));
        assert_eq!(parse(b"#!/bin/sh \0\n"), with_arg(b"/bin/sh", b""));
        // No interpreter.
        assert_eq!(parse(b"#!\n"), None);
        assert_eq!(parse(b"#!  \t\n"), None);
        // Without a newline the NUL padding ends an empty name, which execve fails with ENOENT.
        assert_eq!(parse(b"#!"), interp(b""));
        // Not a script.
        assert_eq!(parse(b"\0asm"), None);
        assert_eq!(parse(b"# !/bin/sh\n"), None);
    }

    #[test]
    fn shebang_lines_past_the_buffer() {
        // The line goes on past `BINPRM_BUF_SIZE`: the argument is cut, which the interpreter
        // can live with, ...
        let mut long = b"#!/bin/sh ".to_vec();
        long.resize(1000, b'x');
        let (interp, arg) = parse_shebang(&long).unwrap();
        assert_eq!(interp, b"/bin/sh");
        assert_eq!(
            arg.unwrap().len(),
            super::BINPRM_BUF_SIZE - 1 - b"#!/bin/sh ".len()
        );
        // ... but not a path that may be cut.
        let mut path = b"#!/".to_vec();
        path.resize(1000, b'p');
        assert_eq!(parse_shebang(&path), None);
        // A path ending right at the buffer's end is cut too.
        let mut path = b"#!/".to_vec();
        path.resize(super::BINPRM_BUF_SIZE, b'p');
        assert_eq!(parse_shebang(&path), None);
        // A shorter file is padded with NULs, which end the path.
        path.truncate(super::BINPRM_BUF_SIZE - 1);
        assert_eq!(parse_shebang(&path).unwrap().0, &path[2..]);
    }
}
