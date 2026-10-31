//! Threads: `clone` for the thread case (through the `thread-spawn` extension), thread groups,
//! `exit` versus `exit_group`, `de_thread` for `execve`, releasing tasks the execution layer is
//! done with, `set_tid_address` and robust futex lists.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    CloneFlags, GuestPtrMut, RobustListHeadPtr, RobustListHeadPtrPtr, S32Ptr, SiCode, Ulong, Uptr,
    Usize, UsizePtr,
};
use wasixv2_syscalls::{
    Caller, CpuAccount, Errno, GuestAction, Image, Resume, SysResult, TaskId, WorkFlag, write_value,
};
use wasixv2_wait::{Interrupted, NoInterrupt, PollMask, WaitQueue, wait_event};

use crate::{Kernel, Process, State, Task, signal::Signals, sync::MutexExt, work::Work};

/// `sizeof(struct robust_list_head)` on ILP32.
const ROBUST_LIST_HEAD_SIZE: u32 = 12;

/// A thread group (a process, in user terms): which of its tasks are still running, and how it
/// ended.
#[derive(Default)]
pub(crate) struct Group {
    state: Mutex<GroupState>,
    /// Woken once the group exits, for its tasks blocked in the kernel, and when an `execve`
    /// hands the leader a new image.
    pub(crate) exited: WaitQueue,
    /// Woken whenever one of the group's tasks is released, and when the group exits.
    released: WaitQueue,
}

#[derive(Default)]
struct GroupState {
    /// Tasks that have not exited yet, in step with their `ThreadState::exited`.
    live: u32,
    /// An `execve` between `de_thread`'s start and its commit (`group_exec_task`).
    exec: Option<GroupExec>,
    /// The status of `exit_group`, or of the group's end when its last task called `exit`.
    exit: Option<i32>,
    /// The status of the leader's own `exit`, which is the group's when it ends without an
    /// `exit_group` (`wait_task_zombie` reads the leader's `exit_code`).
    leader_exit: Option<i32>,
    /// One caller has claimed final group cleanup after all memory users left.
    notified: bool,
}

/// An `execve` in progress in a thread group.
#[derive(Clone, Copy)]
struct GroupExec {
    /// The leader, when it is not the task that calls `execve`, left its old image and waits
    /// for the new one: what Linux's `de_thread` waits for as the leader's `exit_state`.
    leader_parked: bool,
}

impl Group {
    /// A group with its first task.
    pub(crate) fn new() -> Self {
        let group = Self::default();
        group.state.lock_or_recover().live = 1;
        group
    }

    /// The group is exiting or exited.
    pub(crate) fn exiting(&self) -> bool {
        self.state.lock_or_recover().exit.is_some()
    }

    /// `signal_group_exit`: the group is exiting, or one of its tasks is in an `execve` that
    /// is ending the others. It starts no new tasks.
    pub(crate) fn signal_group_exit(&self) -> bool {
        let state = self.state.lock_or_recover();
        state.exit.is_some() || state.exec.is_some()
    }

    /// No task of the group but the caller is still running (`thread_group_empty`).
    pub(crate) fn single_threaded(&self) -> bool {
        self.state.lock_or_recover().live <= 1
    }

    pub(crate) fn status(&self) -> Option<i32> {
        self.state.lock_or_recover().exit
    }
}

/// The per-task thread state.
#[derive(Clone, Copy, Default)]
pub(crate) struct ThreadState {
    /// `clear_child_tid`: zeroed and futex-woken when the task exits.
    clear_child_tid: u32,
    /// `robust_list`: the head the task registered.
    robust_list: u32,
    /// The thread pointer `CLONE_SETTLS` gave the task.
    tls: u32,
    /// The restartable sequences area the task registered (`task_struct::rseq`).
    pub(crate) rseq: Option<crate::rseq::Rseq>,
    /// The task called `exit` or was killed with its thread; it goes away once the execution
    /// layer releases it.
    exited: bool,
    /// The task is done with its memory ([`Kernel::exit_mm`]; Linux clears `tsk->mm`), so it
    /// no longer counts among the memory's users.
    left_mm: bool,
    /// `mmput` and its asynchronous `exit_mmap` finished; only then may exit_notify reap us.
    exit_mm_done: bool,
    /// The task died while it waited for its vfork child, and its `exit_mm` went to the
    /// instance the child runs in ([`GuestAction::ExitMm`]); it goes there once.
    exit_mm_handed: bool,
}

impl ThreadState {
    pub(crate) fn tid_address(&self) -> u32 {
        self.clear_child_tid
    }

    /// The task called `exit` or was killed with its thread.
    pub(crate) fn exited(&self) -> bool {
        self.exited
    }

    /// Whether the task is done with its memory (Linux's `tsk->mm == NULL`).
    pub(crate) fn left_mm(&self) -> bool {
        self.left_mm
    }

    pub(crate) fn exit_mm_done(&self) -> bool {
        self.exit_mm_done
    }

    /// The thread state of a new process's first task, made from the caller's (`copy_process`):
    /// no robust list, the caller's thread pointer unless `CLONE_SETTLS` gives `tls`,
    /// `CLONE_CHILD_CLEARTID`'s `child_tid`, and the caller's rseq area unless the child
    /// shares its memory (`rseq_fork`).
    pub(crate) fn for_process(&self, flags: CloneFlags, tls: u32, child_tid: u32) -> Self {
        Self {
            clear_child_tid: match flags.contains(CloneFlags::CLONE_CHILD_CLEARTID) {
                true => child_tid,
                false => 0,
            },
            robust_list: 0,
            tls: match flags.contains(CloneFlags::CLONE_SETTLS) {
                true => tls,
                false => self.tls,
            },
            rseq: match flags.contains(CloneFlags::CLONE_VM) {
                true => None,
                false => self.rseq,
            },
            exited: false,
            left_mm: false,
            exit_mm_done: false,
            exit_mm_handed: false,
        }
    }
}

/// The flags every thread needs: one address space, filesystem context, descriptor table, signal
/// handlers and thread group. Other sharing combinations are processes, which go through `clone`.
const THREAD: CloneFlags = CloneFlags(
    CloneFlags::CLONE_VM.0
        | CloneFlags::CLONE_FS.0
        | CloneFlags::CLONE_FILES.0
        | CloneFlags::CLONE_SIGHAND.0
        | CloneFlags::CLONE_THREAD.0,
);

/// What a thread may ask for besides `THREAD`. `CLONE_SYSVSEM` is accepted: there is no SysV
/// semaphore undo state yet, and a thread shares it anyway. `CLONE_DETACHED` is ignored, as on
/// Linux; the exit signal (`CSIGNAL`) means nothing for a thread.
const THREAD_OPTIONAL: CloneFlags = CloneFlags(
    CloneFlags::CSIGNAL.0
        | CloneFlags::CLONE_SYSVSEM.0
        | CloneFlags::CLONE_SETTLS.0
        | CloneFlags::CLONE_PARENT_SETTID.0
        | CloneFlags::CLONE_CHILD_SETTID.0
        | CloneFlags::CLONE_CHILD_CLEARTID.0
        | CloneFlags::CLONE_DETACHED.0
        | CloneFlags::CLONE_UNTRACED.0
        | CloneFlags::CLONE_IO.0,
);

impl Kernel {
    /// `copy_process` for a thread: validates `flags`, creates the task in the caller's thread
    /// group, writes its tid where asked, and leaves the start to the execution layer. Without
    /// `CLONE_THREAD`, the task is a new process on the caller's memory ([`Kernel::kernel_clone`]).
    pub(crate) async fn sys_thread_spawn(
        &self,
        caller: &Caller<'_>,
        flags: Ulong,
        start_arg: Uptr,
        parent_tid: S32Ptr,
        tls: Uptr,
        child_tid: S32Ptr,
    ) -> SysResult<i32> {
        let flags = CloneFlags(flags.into());
        // Without `CLONE_THREAD`, a process on the caller's memory, which `clone` makes.
        if !flags.contains(CloneFlags::CLONE_THREAD) {
            if !flags.contains(CloneFlags::CLONE_VM) {
                return Err(Errno::EINVAL.into());
            }
            let args = crate::process::KernelClone {
                flags: CloneFlags(flags.0 & !CloneFlags::CSIGNAL.0),
                exit_signal: flags.0 & CloneFlags::CSIGNAL.0,
                pidfd: parent_tid.addr(),
                parent_tid: parent_tid.addr(),
                child_tid: child_tid.addr(),
                stack: 0,
                tls,
                set_tid: Vec::new(),
                start_arg: Some(start_arg),
                cgroup: 0,
            };
            return self.kernel_clone(caller, args).await;
        }
        // copy_process's checks, in its order.
        if flags.contains(CloneFlags::CLONE_THREAD) && !flags.contains(CloneFlags::CLONE_SIGHAND) {
            return Err(Errno::EINVAL.into());
        }
        if flags.contains(CloneFlags::CLONE_SIGHAND) && !flags.contains(CloneFlags::CLONE_VM) {
            return Err(Errno::EINVAL.into());
        }
        if !flags.contains(THREAD) || flags.0 & !(THREAD.0 | THREAD_OPTIONAL.0) != 0 {
            return Err(Errno::EINVAL.into());
        }
        let parent = caller.task();
        let child = {
            let mut state = self.state.lock_or_recover();
            let task = state
                .tasks
                .get(&parent)
                .expect("syscall from an unknown task");
            let process = task.process.clone();
            // `copy_process`: a thread starts in its group's pid namespace, so none can start
            // once the task's children would go to another.
            if !Arc::ptr_eq(&process.pid_ns, &task.live_ns().pid_for_children) {
                return Err(Errno::EINVAL.into());
            }
            if crate::resource::nproc_exceeded(&state, task) {
                return Err(Errno::EAGAIN.into());
            }
            if state.tasks.len() as i64 >= self.tunables.get(crate::sysctl::Tunable::ThreadsMax) {
                return Err(Errno::EAGAIN.into());
            }
            let cred = self
                .keys
                .copy_thread_creds(task.cred_override.as_ref().unwrap_or(&task.cred))?;
            let sched = task.sched.for_child()?;
            let id = TaskId(state.last_id + 1);
            state.pids.alloc(id.0, &process.pid_ns, &[], |_| false)?;
            // `cgroup_can_fork`: a thread starts in its creator's cgroup.
            let cpu = Arc::new(CpuAccount::new(process.cpu.clone()));
            let nice = sched.nice() > 0;
            let css = match self
                .cgroups
                .fork(task.css.cgroup().clone(), cpu.clone(), nice)
            {
                Ok(css) => css,
                Err(errno) => {
                    state.pids.detach(id.0, crate::pid::PidType::Pid);
                    return Err(errno.into());
                }
            };
            {
                let mut group = process.group.state.lock_or_recover();
                // A group on its way out, or in an `execve`, starts nothing (Linux:
                // `fatal_signal_pending`, the leader's SIGKILL from `de_thread`).
                if group.exit.is_some() || group.exec.is_some() {
                    state.pids.detach(id.0, crate::pid::PidType::Pid);
                    return Err(Errno::EINTR.into());
                }
                group.live += 1;
            }
            let thread = ThreadState {
                clear_child_tid: match flags.contains(CloneFlags::CLONE_CHILD_CLEARTID) {
                    true => child_tid.addr(),
                    false => 0,
                },
                robust_list: 0,
                tls: match flags.contains(CloneFlags::CLONE_SETTLS) {
                    true => tls,
                    false => task.thread.tls,
                },
                // `rseq_fork` with `CLONE_VM`.
                rseq: None,
                exited: false,
                left_mm: false,
                exit_mm_done: false,
                exit_mm_handed: false,
            };
            let flag = Arc::new(WorkFlag::default());
            let new = Task {
                cred,
                cpu,
                css,
                syscall: Default::default(),
                ioac: Default::default(),
                process,
                mm: task.mm.clone(),
                files: task.files.clone(),
                fs: task.fs.clone(),
                ns: task.ns.clone(),
                work: Work::new(flag.clone()),
                restart: None,
                session_keyring_work: None,
                comm: task.comm,
                sched,
                prctl: task.prctl,
                seccomp: task.seccomp.clone(),
                io: crate::sched::copy_io(task.io.as_ref(), flags.contains(CloneFlags::CLONE_IO)),
                thread,
                signals: task
                    .signals
                    .group()
                    .attach(id, task.signals.inherit(THREAD), flag),
                cred_override: None,
                uring: Arc::default(),
            };
            // `cgroup_post_fork`: a task made in a freezing cgroup is frozen before it runs.
            if new.css.freezing() {
                new.signals.freeze(true);
            }
            state.last_id += 1;
            new.signals.publish_cred(new.cred.clone());
            state.tasks.insert(id, new);
            id
        };
        self.ptrace_init_task(parent, child, flags, 0);
        // A thread is in its group's pid namespace, the caller's.
        let tid = self.own_tid(child);
        // Linux ignores faults on both writes.
        if flags.contains(CloneFlags::CLONE_PARENT_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(parent_tid.addr()), &tid).await;
        }
        if flags.contains(CloneFlags::CLONE_CHILD_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(child_tid.addr()), &tid).await;
        }
        let start = GuestAction::StartThread {
            task: child,
            tid,
            start_arg,
        };
        self.queue(parent, start);
        self.ptrace_clone_event(parent).await;
        Ok(tid)
    }

    /// Leaves `action` for the execution layer on `task`'s next return to the guest.
    pub(crate) fn queue(&self, task: TaskId, action: GuestAction) {
        let unstarted = {
            let mut state = self.state.lock_or_recover();
            match state.tasks.get_mut(&task) {
                Some(entry) => entry.work.push(action),
                None => return,
            }
        };
        for child in unstarted {
            self.abort_unstarted(child);
        }
    }

    /// Forgets `child`, a task that was to start and never will: a thread, or the first task of
    /// a process `clone` made, whose process also leaves the tree, as `copy_process` undoes a
    /// child that cannot run (`bad_fork_*`).
    fn abort_unstarted(&self, child: TaskId) {
        let process = {
            let state = self.state.read_or_recover();
            match state.tasks.get(&child) {
                Some(entry) => entry.process.clone(),
                None => return,
            }
        };
        if process.pid == child {
            self.exit_files(&process);
            let vfork_parent = {
                let mut tree = self.procs.tree.lock_or_recover();
                tree.reap(child.0);
                tree.vfork_pids.remove(&child);
                tree.vforks.remove(&child)
            };
            if let Some(parent) = vfork_parent {
                self.vfork_done(parent);
            }
        }
        let stopped = forget_task(&mut self.state.lock_or_recover(), child);
        self.complete_vfork_done(child);
        self.report_group_stop(stopped);
        self.pid_ns_progress();
    }

    /// `do_exit` for one task: its memory goes first ([`Kernel::exit_mm`]). The last task to
    /// exit ends its thread group, with the leader's status. The group leader stays until then,
    /// as a zombie leader does on Linux, so the group's status reaches whoever started the
    /// process.
    pub(crate) async fn sys_exit(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let task = caller.task();
        // `do_exit`'s `ptrace_event(PTRACE_EVENT_EXIT, code)`, with the status as wait has it.
        let event = wasixv2_abi::PtraceEvent::PTRACE_EVENT_EXIT;
        self.task_signals(task)
            .group()
            .with_trace(task, |t| t.exit_reported = true);
        self.ptrace_event(task, event, ((status & 0xff) << 8) as u64)
            .await;
        let process = self.process(task);
        self.exit_mm(caller).await;
        let last = {
            let mut state = self.state.lock_or_recover();
            let entry = state
                .tasks
                .get_mut(&task)
                .expect("syscall from an unknown task");
            // Killed meanwhile by another thread's `execve`, which leaves an exit pending, or
            // an `execve` handed the leader its image.
            if entry.thread.exited || entry.work.has_exec() {
                return Ok(());
            }
            entry.thread.exited = true;
            let mut group = process.group.state.lock_or_recover();
            if task == process.pid {
                group.leader_exit = Some(status & 0xff);
            }
            group.live -= 1;
            (group.live == 0).then(|| group.leader_exit.unwrap_or(status))
        };
        if let Some(status) = last {
            self.do_group_exit(task, status);
            return Ok(());
        }
        if task == process.pid {
            // Until the group exits, or another thread's `execve` gives the leader its image.
            let status = wait_event(
                &process.group.exited,
                || match process.group.status() {
                    Some(status) => Some(Some(status)),
                    None => self.with_task(task, |t| t.work.has_exec()).then_some(None),
                },
                &NoInterrupt,
            )
            .await
            .expect("NoInterrupt never interrupts");
            if let Some(status) = status {
                self.queue(task, GuestAction::Exit(status));
            }
            return Ok(());
        }
        self.queue(task, GuestAction::Exit(status & 0xff));
        Ok(())
    }

    /// `do_group_exit`, then the caller's own `exit_mm`. The other tasks leave the memory each
    /// in its own exit, once it stopped running, so a robust lock one takes after the call is
    /// released too.
    pub(crate) async fn sys_exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let event = wasixv2_abi::PtraceEvent::PTRACE_EVENT_EXIT;
        self.task_signals(caller.task())
            .group()
            .with_trace(caller.task(), |t| t.exit_reported = true);
        self.ptrace_event(caller.task(), event, ((status & 0xff) << 8) as u64)
            .await;
        self.do_group_exit(caller.task(), status);
        self.exit_mm(caller).await;
        Ok(())
    }

    /// `do_group_exit`: every task of the group exits with `status`. The first exit wins. Tasks
    /// blocked in an interruptible wait notice; the others unwind on their next return to the
    /// guest or preemption point, each through its own [`Kernel::exit_mm`].
    pub(crate) fn do_group_exit(&self, task: TaskId, status: i32) {
        // Linux keeps only the low byte of the status.
        let status = status & 0xff;
        let mut state = self.state.lock_or_recover();
        let process = state.tasks[&task].process.clone();
        let status = *process
            .group
            .state
            .lock_or_recover()
            .exit
            .get_or_insert(status);
        let mut others = false;
        let mut unstarted = Vec::new();
        for (&id, t) in state.tasks.iter_mut() {
            if Arc::ptr_eq(&t.process, &process) {
                unstarted.extend(t.work.push(GuestAction::Exit(status)));
                others |= id != task;
            }
        }
        for child in unstarted {
            forget_task(&mut state, child);
        }
        let signals = state.tasks[&task].signals.group().clone();
        drop(state);
        // `SIGNAL_GROUP_EXIT`: no task stops any more, and stopped ones go on to their exit.
        signals.cancel_stop(true);
        if others {
            signals.kill_all();
        }
        // The dying group's timers stop (`do_exit` with `group_dead`).
        signals.cancel_real_timer();
        crate::posix_timer::exit_itimers(&signals);
        process.group.exited.wake(PollMask::empty());
        process.group.released.wake(PollMask::empty());
        self.exit_notify(&process);
        self.pid_ns_progress();
    }

    /// `exit_notify` once the group has exited, none of its threads is left but the leader
    /// (`thread_group_empty`, `delay_group_leader`), and the leader is done with its memory
    /// (its `do_exit` ran `exit_mm` before `exit_notify`): only then the process becomes a
    /// zombie, its files close and its parent hears of it, so nothing of it runs after its exit
    /// is seen, and its shared mappings are written back by then. A vfork child's leader is the
    /// exception: its memory is its parent's, which it never writes back, and its end runs in
    /// the parent's instance as the parent's return ([`GuestAction::VforkReturn`]), which
    /// releases its memory there. A task waiting for its vfork child whose image ended is gone
    /// once it left its memory, as Linux's killable `wait_for_vfork_done` lets it exit: its
    /// instance runs the child, which leaves the memory for it ([`GuestAction::ExitMm`]), and
    /// once the child is done goes back to the task only to unwind it. A task waiting for its
    /// vfork child otherwise counts as running.
    pub(crate) fn exit_notify(&self, process: &Arc<Process>) {
        let Some(status) = process.group.status() else {
            return;
        };
        let waiting = self.vfork_parents();
        let vfork_parent = self.procs.tree.lock_or_recover().vfork_parent(process.pid);
        let (others, leader_runs, hand_back) = {
            let state = self.state.lock_or_recover();
            let gone = |id: &TaskId, t: &Task| gone_in_vfork(&waiting, *id, t);
            let others = state.tasks.iter().any(|(id, t)| {
                *id != process.pid && Arc::ptr_eq(&t.process, process) && !gone(id, t)
            });
            let leader_runs = state.tasks.get(&process.pid).is_some_and(|t| {
                if waiting.contains(&process.pid) {
                    !gone(&process.pid, t)
                } else {
                    Arc::ptr_eq(&t.process, process) && !t.thread.exit_mm_done
                }
            });
            // A vfork child's leader that is done but for leaving the memory hands its parent's
            // instance back, once, so the instance runs its `exit_mm` (and so its
            // `complete_vfork_done`) there before it can be a zombie.
            let hand_back = vfork_parent.filter(|_| {
                !others
                    && state.tasks.get(&process.pid).is_some_and(|t| {
                        Arc::ptr_eq(&t.process, process)
                            && !t.thread.left_mm
                            && !t.work.has_vfork_return()
                    })
            });
            (others, leader_runs, hand_back)
        };
        if let Some(parent) = hand_back {
            self.hand_back(process.pid, parent);
        }
        if !others && !leader_runs {
            // ---- ns ----
            // A pid namespace's init is a zombie only once the namespace is empty.
            if self.zap_pid_ns(process) {
                return;
            }
            {
                // kernel/exit.c:do_exit elects group_dead once with
                // atomic_dec_and_test(signal->live). Multiple host releases can
                // reach this point together; only one may publish/reap the group.
                let mut group = process.group.state.lock_or_recover();
                if group.notified {
                    return;
                }
                group.notified = true;
            }
            // exit_mm releases mm->exe_file before the process becomes a zombie.
            process.exe_write_access.lock_or_recover().take();
            // ---- ipc ----
            // `exit_sem`, by the group's last task.
            self.ipc_process_exited(process);
            // ---- proc ----
            // `exit_notify` comes after every task's `exit_mm` (`do_exit`): a zombie is done
            // with its memory, a vfork child's `complete_vfork_done` included.
            debug_assert!(
                self.state
                    .read_or_recover()
                    .tasks
                    .values()
                    .filter(|t| Arc::ptr_eq(&t.process, process))
                    .all(|t| t.thread.exit_mm_done),
                "process {} is a zombie before its tasks left their memory",
                process.pid.0
            );
            // `acct_process`, after the last task's `exit_mm`.
            self.acct_exit(process, status);
            self.process_exited(process, status);
        }
    }

    /// `release_task` for the processes reaped since the last call (`__exit_signal`,
    /// `__unhash_process`, kernel/exit.c): their tasks leave the task table, so nothing finds
    /// them, and what they held, their process first, goes; what the execution layer still
    /// asks of their instances stays as [`DeadTask`]s until it releases them. Called once the
    /// process tree is unlocked, as the task table comes first.
    pub(crate) fn unhash_reaped(&self) {
        let reaped = std::mem::take(&mut self.procs.tree.lock_or_recover().unhash);
        if reaped.is_empty() {
            return;
        }
        let mut gone = Vec::new();
        {
            // A group's reap must not reap its tracer-owned thread zombies.
            // Lock order remains tracees before the task table, as ptrace_link_task.
            let tracees = self.tracees.lock();
            let mut state = self.state.lock_or_recover();
            for pid in &reaped {
                state.retired.remove(&TaskId(*pid));
            }
            let ids: Vec<TaskId> = state
                .tasks
                .iter()
                .filter(|(id, t)| {
                    reaped.contains(&t.process.pid.0)
                        && (**id == t.process.pid || !tracees.contains_key(id))
                })
                .map(|(&id, _)| id)
                .collect();
            for id in ids {
                let Some((mut entry, _)) = unhash_task(&mut state, id) else {
                    continue;
                };
                let flag = entry.work.flag().clone();
                let dead = DeadTask {
                    work: std::mem::replace(&mut entry.work, Work::new(flag)),
                    signals: entry.signals.clone(),
                    cpu: entry.cpu.clone(),
                    cred: entry.cred.clone(),
                    process: Arc::downgrade(&entry.process),
                };
                state.dead.insert(id, dead);
                gone.push(entry);
            }
        }
        // What the tasks held goes with the table unlocked.
        drop(gone);
        for pid in reaped {
            self.yama.task_free(pid);
        }
    }

    /// `release_task`: the execution layer is done with `task`, so it goes. A zombie process
    /// lives on in the process tree. For a task its reaper took out of the table already, this
    /// is `put_task_struct`: what was left of it goes.
    pub(crate) fn release_task(&self, task: TaskId) {
        if let Some(process) = self.defer_traced_thread_release(task) {
            self.exit_notify(&process);
            return;
        }
        let dead = self.state.lock_or_recover().dead.remove(&task);
        if let Some(mut dead) = dead {
            let unstarted = dead.work.take_unstarted();
            let mut state = self.state.lock_or_recover();
            for child in unstarted {
                forget_task(&mut state, child);
            }
            drop(state);
            if let Some(process) = dead.process.upgrade() {
                process.group.released.wake(PollMask::empty());
            }
            return;
        }
        // fs/fuse/dev.c:request_wait_answer and file.c:fuse_flush/fuse_file_put:
        // fault/fatal teardown must not wait on the daemon. Normal exit_mm closed these
        // files already, using the async flush path.
        self.exit_files_fatal(task);
        // A fork child that never ran has no `exit_mm` to let its vfork parent go.
        self.complete_vfork_done(task);
        let (process, stopped) = {
            let mut state = self.state.lock_or_recover();
            let (unstarted, process) = match state.tasks.get_mut(&task) {
                Some(entry) => (entry.work.take_unstarted(), entry.process.clone()),
                None => return,
            };
            if task == process.pid {
                let entry = &state.tasks[&task];
                let retired = RetiredTask {
                    cred: entry.cred.clone(),
                    sched: entry.sched.clone(),
                };
                state.retired.insert(task, retired);
            }
            let mut stopped = None;
            for child in unstarted {
                stopped = stopped.or(forget_task(&mut state, child));
            }
            stopped = stopped.or(forget_task(&mut state, task));
            (process, stopped)
        };
        self.report_group_stop(stopped);
        self.exit_notify(&process);
        self.pid_ns_progress();
        if self.procs.find(process.pid.0).is_none() {
            self.state.lock_or_recover().retired.remove(&process.pid);
        }
    }

    /// Tells the parent of the group stop that [`forget_task`] completed for `process`.
    pub(crate) fn report_group_stop(&self, process: Option<Arc<Process>>) {
        let Some(process) = process else {
            return;
        };
        let status = self
            .live_signals(&process)
            .map_or(0, |signals| signals.stop_status());
        self.notify_parent_cldstop(&process, SiCode::CLD_STOPPED, status);
    }

    /// The signal state of `process`'s thread group, while it has a task.
    fn live_signals(&self, process: &Arc<Process>) -> Option<Arc<Signals>> {
        let state = self.state.read_or_recover();
        state
            .tasks
            .values()
            .find(|t| Arc::ptr_eq(&t.process, process))
            .map(|t| t.signals.group().clone())
    }

    /// `thread-spawn` made `task`, but its thread never started. The first task of a process
    /// on the caller's memory takes its process with it, as `copy_process` undoes a child.
    pub(crate) fn abort_thread(&self, task: TaskId) {
        let (process, stopped) = {
            let mut state = self.state.lock_or_recover();
            let Some(process) = state.tasks.get(&task).map(|t| t.process.clone()) else {
                return;
            };
            if process.pid == task {
                drop(state);
                self.abort_unstarted(task);
                return;
            }
            (process, forget_task(&mut state, task))
        };
        self.report_group_stop(stopped);
        self.exit_notify(&process);
        self.pid_ns_progress();
    }

    /// Sends SIGKILL to every process of the machine but the kernel's init, as `kill(-1,
    /// SIGKILL)` from outside it does: each thread group dies at its next interruption point.
    pub fn kill_all(&self) {
        for (_, signals) in self.live_processes() {
            signals.kill_all();
        }
    }

    /// Ends every process of the machine, as powering it off does: [`Kernel::kill_all`], then
    /// waits until the execution layer has released every task, so nothing of the machine runs
    /// any more. A process forked meanwhile is killed too.
    pub async fn terminate(&self) {
        loop {
            let processes = self.kill_everything();
            if processes.is_empty() {
                return;
            }
            for process in processes {
                let gone = || {
                    let state = self.state.lock_or_recover();
                    // A reaped process's tasks count until their instances are released.
                    let left = state
                        .tasks
                        .values()
                        .any(|t| Arc::ptr_eq(&t.process, &process))
                        || state
                            .dead
                            .values()
                            .any(|d| std::ptr::eq(d.process.as_ptr(), Arc::as_ptr(&process)));
                    (!left).then_some(())
                };
                wait_event(&process.group.released, gone, &NoInterrupt)
                    .await
                    .expect("NoInterrupt never interrupts");
            }
        }
    }

    /// Sends SIGKILL to every process of the machine but the kernel's init, and returns them.
    pub(crate) fn kill_everything(&self) -> Vec<Arc<Process>> {
        let processes = self.live_processes();
        for (_, signals) in &processes {
            signals.kill_all();
        }
        processes.into_iter().map(|(process, _)| process).collect()
    }

    /// Every process that has a task, with its thread group's signals; not the kernel's init,
    /// which has no instance to end.
    fn live_processes(&self) -> Vec<(Arc<Process>, Arc<Signals>)> {
        let state = self.state.lock_or_recover();
        let mut processes: Vec<(Arc<Process>, Arc<Signals>)> = Vec::new();
        for task in state.tasks.values() {
            if self.is_init(&task.process) {
                continue;
            }
            if !processes.iter().any(|(p, _)| Arc::ptr_eq(p, &task.process)) {
                processes.push((task.process.clone(), task.signals.group().clone()));
            }
        }
        processes
    }

    /// Waits until `task` is the only task of its thread group.
    pub(crate) async fn group_wait(&self, task: TaskId) {
        // A task its reaper took out of the table was its group's last.
        let process = {
            let state = self.state.read_or_recover();
            match state.tasks.get(&task) {
                Some(entry) => entry.process.clone(),
                None => return,
            }
        };
        let alone = || {
            let state = self.state.lock_or_recover();
            let others = state
                .tasks
                .iter()
                .any(|(&id, t)| id != task && Arc::ptr_eq(&t.process, &process));
            (!others).then_some(())
        };
        wait_event(&process.group.released, alone, &NoInterrupt)
            .await
            .expect("NoInterrupt never interrupts")
    }

    /// `de_thread`'s start at the point of no return of `execve` from `task`
    /// (`zap_other_threads`): every other task of the thread group but the leader exits and is
    /// sent SIGKILL, so blocked ones wake up. Each leaves the memory in its own exit
    /// ([`Kernel::exit_mm`]), before it is gone ([`Kernel::de_thread`]). Returns the guard of
    /// the `execve`, which the group is in until it goes. `EAGAIN` when the group is exiting or
    /// another task's `execve` got there first.
    pub(crate) fn zap_threads(&self, task: TaskId) -> Result<ExecGuard, Errno> {
        let mut state = self.state.lock_or_recover();
        let process = state.tasks[&task].process.clone();
        let leader = process.pid;
        {
            let mut group = process.group.state.lock_or_recover();
            if group.exit.is_some() || group.exec.is_some() {
                return Err(Errno::EAGAIN);
            }
            group.exec = Some(GroupExec {
                leader_parked: false,
            });
        }
        let mut unstarted = Vec::new();
        let mut killed = Vec::new();
        let mut ended = 0;
        for (&id, t) in state.tasks.iter_mut() {
            if !Arc::ptr_eq(&t.process, &process) || id == task || id == leader {
                continue;
            }
            if !t.thread.exited {
                t.thread.exited = true;
                ended += 1;
            }
            unstarted.extend(t.work.push(GuestAction::Exit(0)));
            killed.push(t.signals.clone());
        }
        process.group.state.lock_or_recover().live -= ended;
        for child in unstarted {
            forget_task(&mut state, child);
        }
        let group = state.tasks[&task].signals.group().clone();
        drop(state);
        // No task stops any more, and a stopped leader goes on to take the new image.
        group.cancel_stop(false);
        for signals in killed {
            signals.kill();
        }
        Ok(ExecGuard(process))
    }

    /// `de_thread`'s wait, after [`Kernel::zap_threads`]: until the tasks it killed are gone and
    /// the leader, when it is not `task`, has left its old image ([`Kernel::exec_wait`]), so no
    /// thread runs the old image but `task` when the `execve` commits. A leader that waits for
    /// its vfork child left its old image once it left its memory, as Linux's `de_thread` waits
    /// for the leader killed in `wait_for_vfork_done` to be a zombie, not for the child; and
    /// another thread that waits for its vfork child is gone once it left its memory.
    /// Killable, as Linux's (`TASK_KILLABLE`): a fatal signal ends the group, and the `execve`
    /// with EAGAIN; so does an exit of the group meanwhile.
    pub(crate) async fn de_thread(&self, task: TaskId) -> Result<(), Errno> {
        let process = self.process(task);
        let leader = process.pid;
        let signals = self.task_signals(task);
        let done = || {
            let waiting = self.vfork_parents();
            let state = self.state.read_or_recover();
            if state.tasks[&task].work.exit().is_some() {
                return Some(false);
            }
            // A thread killed while it waited for its vfork child is gone once it left its
            // memory, whose instance runs the child on (`zap_other_threads` kills it in
            // `wait_for_vfork_done`, which is killable).
            let others = state.tasks.iter().any(|(&id, t)| {
                id != task
                    && id != leader
                    && Arc::ptr_eq(&t.process, &process)
                    && !gone_in_vfork(&waiting, id, t)
            });
            // A leader that is a vfork child itself takes the image where its own vfork
            // parent waits (`Exec { vfork_parent }`), so it does not leave its old image
            // before its child is done.
            let left = state
                .tasks
                .get(&leader)
                .is_some_and(|t| t.thread.exit_mm_done && !t.work.exec_resumes_vfork_parent());
            let dead_in_vfork = left && self.vfork_waits(leader);
            let group = process.group.state.lock_or_recover();
            let parked = task == leader
                || dead_in_vfork
                || group.exec.is_some_and(|exec| exec.leader_parked);
            (!others && parked).then_some(true)
        };
        let fatal = match wait_event(&process.group.released, done, &signals.killable()).await {
            Ok(true) => return Ok(()),
            Ok(false) => None,
            Err(Interrupted) => signals.fatal_pending(),
        };
        // `complete_signal`: a fatal signal ends the whole group as it arrives.
        if let Some(sig) = fatal {
            self.force_fatal_signal(task, sig);
        }
        Err(Errno::EAGAIN)
    }

    /// `de_thread`'s end, once the old image is gone from every thread but `task`: the new
    /// image runs as the group leader, which takes over `task`'s signal state when it is not
    /// `task` itself (Linux exchanges their pids), and `task` exits. A leader that waits for
    /// its vfork child waits no more: its instance goes on running the child and ends with it
    /// ([`Resume::Gone`]), and `task` starts the new image in an instance of its own
    /// ([`GuestAction::StartImage`]) before it exits. Returns the leader.
    pub(crate) fn exec_leader(&self, task: TaskId) -> TaskId {
        let mut state = self.state.lock_or_recover();
        let process = state.tasks[&task].process.clone();
        let leader = process.pid;
        let mut unstarted = Vec::new();
        if task != leader {
            let nested = state.tasks[&leader].work.exec_resumes_vfork_parent();
            let detached = !nested && {
                let mut tree = self.procs.tree.lock_or_recover();
                let child = tree.waited_for_by(leader);
                if let Some(child) = child {
                    tree.vforks.remove(&child);
                    tree.detached.insert(child, leader);
                }
                child.is_some()
            };
            let group = state.tasks[&leader].signals.group().clone();
            group.take_over(task, leader);
            let entry = state.tasks.get_mut(&task).expect("the caller is listed");
            entry.thread.exited = true;
            if detached {
                unstarted.extend(entry.work.push(GuestAction::StartImage { task: leader }));
            }
            unstarted.extend(entry.work.push(GuestAction::Exit(0)));
            let [Some(entry), Some(old)] = state.tasks.get_disjoint_mut([&leader, &task]) else {
                unreachable!("the leader and the caller are listed");
            };
            exchange_tasks(entry, old);
            std::mem::swap(&mut entry.syscall, &mut old.syscall);
            // fs/exec.c de_thread retires the old leader and keeps the caller accounting.
            process.ioac.add(entry.ioac.counts());
            entry.ioac = std::mem::take(&mut old.ioac);
            std::mem::swap(&mut entry.prctl, &mut old.prctl);
            if detached {
                entry.work.detach_exec();
            }
            unstarted.extend(entry.work.take_unstarted());
            entry.thread.exited = false;
            entry.restart = None;
            // The survivor keeps its place in the cgroup hierarchy, as it keeps its `css_set`
            // when Linux exchanges the two tasks' pids.
            crate::cgroup::Css::exchange(&state.tasks[&task].css, &state.tasks[&leader].css);
        }
        for child in unstarted {
            forget_task(&mut state, child);
        }
        process.group.state.lock_or_recover().live = 1;
        drop(state);
        if task != leader {
            self.ptrace_exec_leader(task, leader);
        }
        leader
    }

    /// Leaves the image of an `execve` for the leader `task`, and wakes it wherever it waits,
    /// a zombie leader's wait for its group included. A vfork child's instance goes back to the
    /// parent once it takes the image; the parent returns from vfork unless its image ended.
    /// A leader that waits for its own vfork child is dead to its old image, as `de_thread`
    /// kills it in `wait_for_vfork_done`: the child, which runs in its instance, is woken to
    /// leave the leader's memory for it ([`GuestAction::ExitMm`]), and the image starts in
    /// another instance once the `execve` is done ([`Kernel::exec_leader`]).
    pub(crate) fn hand_image(&self, task: TaskId, image: Image) {
        let vfork_parent = self.procs.tree.lock_or_recover().end_vfork(task);
        if let Some(parent) = vfork_parent
            && parent.resumes != Resume::Gone
        {
            self.ptrace_vfork_done(parent.task, task);
            self.vfork_done(parent.task);
        }
        let (process, signals) = {
            let mut state = self.state.lock_or_recover();
            let entry = state.tasks.get_mut(&task).expect("the leader is listed");
            let unstarted = entry.work.exec(image, vfork_parent);
            let handed = (entry.process.clone(), entry.signals.clone());
            for child in unstarted {
                forget_task(&mut state, child);
            }
            handed
        };
        signals.group().notify(task);
        process.group.exited.wake(PollMask::empty());
        self.notify_vfork_child(task);
    }

    /// Before the image `execve` loaded starts in `task`'s instance, which left the old one:
    /// waits until the `execve` committed and `task` is the only task of its thread group, but
    /// for threads killed in vfork whose instances run their children on ([`gone_in_vfork`]).
    /// `Err(status)` when the group exited meanwhile; its exit is taken.
    pub(crate) async fn exec_wait(&self, task: TaskId) -> Result<(), i32> {
        let process = self.process(task);
        // An `execve` from another thread waits for the leader's old image to be gone.
        if let Some(exec) = &mut process.group.state.lock_or_recover().exec {
            exec.leader_parked = true;
        }
        process.group.released.wake(PollMask::empty());
        let alone = || {
            let waiting = self.vfork_parents();
            let mut state = self.state.lock_or_recover();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return Some(Err(0));
            };
            if let Some(status) = entry.work.exit() {
                while entry.work.take().is_some() {}
                return Some(Err(status));
            }
            if process.group.state.lock_or_recover().exec.is_some() {
                return None;
            }
            let others = state.tasks.iter().any(|(&id, t)| {
                id != task && Arc::ptr_eq(&t.process, &process) && !gone_in_vfork(&waiting, id, t)
            });
            (!others).then_some(Ok(()))
        };
        let result = wait_event(&process.group.released, alone, &NoInterrupt)
            .await
            .expect("NoInterrupt never interrupts");
        if result.is_ok() {
            let old_vpid = self
                .task_signals(task)
                .group()
                .with_trace(task, |trace| trace.exec_event.take())
                .flatten();
            if let Some(old_vpid) = old_vpid {
                self.ptrace_exec_event(task, old_vpid).await;
            }
        }
        result
    }

    /// The futex state of every task of `task`'s thread group that has not exited, with its
    /// tid. Taken all at once: a task that exits meanwhile can be released, and gone with its
    /// tid, before the caller reaches it.
    pub(crate) fn take_group_futex_states(&self, task: TaskId) -> Vec<(u32, ThreadState)> {
        let mut state = self.state.lock_or_recover();
        let process = state.tasks[&task].process.clone();
        state
            .tasks
            .iter_mut()
            .filter(|(_, t)| Arc::ptr_eq(&t.process, &process) && !t.thread.exited)
            .map(|(&id, t)| (self.own_tid(id) as u32, take_futex_state(t)))
            .collect()
    }

    /// `exec_mm_release` for the futex state of `threads`, tasks of the caller's group that
    /// stopped running the old image: their robust and PI futexes go to their waiters, and a
    /// `clear_child_tid` is only written when the memory outlives the group (a vfork relative),
    /// as Linux writes it only while others use the memory.
    pub(crate) async fn exit_futexes(&self, caller: &Caller<'_>, threads: Vec<(u32, ThreadState)>) {
        let clear_tid = self.mm_shared_elsewhere(caller.task());
        for (tid, thread) in threads {
            self.exit_thread(caller, tid, thread, clear_tid).await;
        }
    }

    /// `futex_exit_release` and `mm_release` for the task whose tid is `tid`, on the memory it
    /// runs on: releases the robust futexes it holds and hands its PI futexes on, then, with
    /// `clear_tid`, zeroes its `clear_child_tid` and wakes a waiter there.
    async fn exit_thread(
        &self,
        caller: &Caller<'_>,
        tid: u32,
        thread: ThreadState,
        clear_tid: bool,
    ) {
        if thread.robust_list != 0 {
            self.exit_robust_list(caller, thread.robust_list, tid).await;
        }
        self.exit_pi_state_list(caller, tid).await;
        if clear_tid && thread.clear_child_tid != 0 {
            let ptr = GuestPtrMut::<u32>::new(thread.clear_child_tid);
            if write_value(caller.memory(), ptr, &0).await.is_ok() {
                // A shared `FUTEX_WAKE`, as `mm_release` makes it.
                let key = self.futex_key(caller.task(), thread.clear_child_tid, false);
                let _ = self.futexes.wake(key, 1, u32::MAX);
            }
        }
    }

    /// `exit_mm` for the caller, which is done with its memory: `mm_release` hands on its
    /// robust and PI futexes, while another task still uses the memory (`mm_users > 1`: a
    /// thread of its group, or a vfork relative) zeroes its `clear_child_tid` and wakes a
    /// waiter there, and lets a parent waiting for it as a vfork child go on; then `mmput`, where the last task to leave the memory writes back its
    /// shared mappings and drops them (`exit_mmap`). Once per task and memory; a later call does
    /// nothing.
    pub(crate) async fn exit_mm(&self, caller: &Caller<'_>) {
        let task = caller.task();
        // A task that took a signal that dumps core dumps while the memory is still there.
        self.coredump_if_dumper(caller).await;
        let (thread, users) = {
            let mut state = self.state.lock_or_recover();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return;
            };
            if entry.thread.left_mm {
                return;
            }
            let thread = take_futex_state(entry);
            (thread, mm_users(&state, task))
        };
        // `do_exit`'s `io_uring_files_cancel`, while the requests' memory is still there.
        self.io_uring_files_cancel(task);
        let tid = self.own_tid(task) as u32;
        self.exit_thread(caller, tid, thread, users > 1).await;
        // The end of `mm_release`.
        self.complete_vfork_done(task);
        self.exit_files_async(caller).await;
        let (last, process) = {
            let mut state = self.state.lock_or_recover();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return;
            };
            entry.thread.left_mm = true;
            // `exit_task_namespaces`, after `exit_mm`: a task that is done holds no namespace,
            // which goes with its last task before the process is seen to exit.
            let ns = entry.ns.take();
            // `cgroup_exit`: it populates its cgroup no more.
            entry.css.exit();
            let process = entry.process.clone();
            let last = mm_users(&state, task) == 0;
            drop(state);
            drop(ns);
            (last, process)
        };
        // `do_exit`'s `setmax_mm_hiwater_rss`, for `getrusage` of the group and its parent.
        self.mem_size(caller).await;
        process.note_rss(self.mm(task).size());
        if last {
            self.exit_mmap(caller).await;
        }
        // kernel/exit.c:do_exit completes exit_mm (including mmput) before exit_notify.
        // Clearing the mm-user count alone must not let a sibling's release reap this task
        // while note_rss or exit_mmap still uses its task-table state.
        self.with_task(task, |t| t.thread.exit_mm_done = true);
        // A leader whose group exited was all its exit waited for, and so was a task that
        // died waiting for its vfork child, whose instance runs on.
        if task == process.pid || self.vfork_waits(task) {
            self.exit_notify(&process);
            process.group.released.wake(PollMask::empty());
        }
        // The accounting records of the exit, while the task can still wait.
        self.acct_flush().await;
        self.ptrace_finish_exit(task);
    }

    /// The vfork parent of `child`, the task its instance runs as, if that parent died while
    /// it waited and has yet to leave its memory: its `exit_mm` goes to this instance, once
    /// ([`GuestAction::ExitMm`]). Linux's parent leaves the memory in its own `do_exit`, before
    /// it is a zombie, while the child keeps the memory (`wait_for_vfork_done` is killable).
    pub(crate) fn dead_vfork_parent(&self, child: TaskId) -> Option<TaskId> {
        // Most tasks are no vfork child: they take no lock but the tree's.
        if !self
            .procs
            .tree
            .lock_or_recover()
            .vforks
            .contains_key(&child)
        {
            return None;
        }
        // Looked up again with the task table locked, as `exec_leader` moves the child
        // between the two under it.
        let mut state = self.state.lock_or_recover();
        let parent = self
            .procs
            .tree
            .lock_or_recover()
            .vforks
            .get(&child)
            .copied()?;
        let entry = state.tasks.get_mut(&parent)?;
        let thread = &mut entry.thread;
        if !entry.work.ends_image() || thread.left_mm || thread.exit_mm_handed {
            return None;
        }
        thread.exit_mm_handed = true;
        Some(parent)
    }

    pub(crate) fn sys_set_tid_address(&self, caller: &Caller<'_>, tidptr: S32Ptr) -> i32 {
        self.with_task(caller.task(), |t| t.thread.clear_child_tid = tidptr.addr());
        self.own_tid(caller.task())
    }

    pub(crate) fn sys_set_robust_list(
        &self,
        caller: &Caller<'_>,
        head: RobustListHeadPtr,
        len: Usize,
    ) -> SysResult<i32> {
        if len != ROBUST_LIST_HEAD_SIZE {
            return Err(Errno::EINVAL.into());
        }
        self.with_task(caller.task(), |t| t.thread.robust_list = head.addr());
        Ok(0)
    }

    /// Only for tasks of the caller's own thread group: there are no credentials to check a
    /// `ptrace` access against yet.
    pub(crate) async fn sys_get_robust_list(
        &self,
        caller: &Caller<'_>,
        pid: i32,
        head_ptr: RobustListHeadPtrPtr,
        len_ptr: UsizePtr,
    ) -> SysResult<i32> {
        // `find_task_by_vpid`.
        let target = match pid {
            0 => Some(caller.task()),
            pid => self.find_vpid(caller, pid).map(TaskId),
        };
        let head = {
            let state = self.state.lock_or_recover();
            let me = &state.tasks[&caller.task()];
            let target = target
                .and_then(|id| state.tasks.get(&id))
                .filter(|t| pid == 0 || !t.thread.exited);
            match target {
                None => return Err(Errno::ESRCH.into()),
                Some(t) if !Arc::ptr_eq(&t.process, &me.process) => {
                    return Err(Errno::EPERM.into());
                }
                Some(t) => t.thread.robust_list,
            }
        };
        write_value(caller.memory(), len_ptr, &ROBUST_LIST_HEAD_SIZE).await?;
        write_value(caller.memory(), head_ptr, &RobustListHeadPtr::new(head)).await?;
        Ok(0)
    }
}

/// The `execve` [`Kernel::zap_threads`] started in a thread group (Linux's `group_exec_task`),
/// until its commit or its failure, or until the call is dropped: the group may exit, exec and
/// start tasks again once this goes.
pub(crate) struct ExecGuard(Arc<Process>);

impl Drop for ExecGuard {
    fn drop(&mut self) {
        let group = &self.0.group;
        group.state.lock_or_recover().exec = None;
        group.released.wake(PollMask::empty());
    }
}

/// The futex state `task` leaves behind, which its exit releases once: Linux clears
/// `robust_list` and `clear_child_tid` as it handles them.
fn take_futex_state(task: &mut Task) -> ThreadState {
    let thread = task.thread;
    (task.thread.robust_list, task.thread.clear_child_tid) = (0, 0);
    thread
}

/// `de_thread`'s `exchange_tids` for a task table that keeps the leader's id: `leader` is the
/// exec'ing thread's task from now on, which carries its namespaces, filesystem context,
/// descriptor table, scheduling and seccomp state into the new image, and `caller`, which
/// exits, is the old leader's, whose `exit_files`, `exit_fs` and `exit_task_namespaces` let
/// the old leader's go. A leader that died waiting for its vfork child let its namespaces go
/// at its `exit_mm` already: they belong to the task that exits, never to its id.
fn exchange_tasks(leader: &mut Task, caller: &mut Task) {
    std::mem::swap(&mut leader.cred, &mut caller.cred);
    std::mem::swap(
        &mut leader.session_keyring_work,
        &mut caller.session_keyring_work,
    );
    std::mem::swap(&mut leader.ns, &mut caller.ns);
    std::mem::swap(&mut leader.fs, &mut caller.fs);
    std::mem::swap(&mut leader.files, &mut caller.files);
    std::mem::swap(&mut leader.sched, &mut caller.sched);
    std::mem::swap(&mut leader.seccomp, &mut caller.seccomp);
}

/// `mm_users` of `task`'s memory: the tasks on it, of any process, that have not left it.
fn mm_users(state: &State, task: TaskId) -> usize {
    let mm = &state.tasks[&task].mm;
    state
        .tasks
        .values()
        .filter(|t| Arc::ptr_eq(&t.mm, mm) && !t.thread.left_mm)
        .count()
}

/// Whether `t`, listed as `id`, is gone from its thread group though its instance runs on: it
/// died while it waited for its vfork child (`waiting` lists the tasks that wait), and left
/// its memory there ([`GuestAction::ExitMm`]), as Linux's killable `wait_for_vfork_done` lets
/// it exit. The instance goes back to it only to unwind it, once the child is done.
fn gone_in_vfork(waiting: &[TaskId], id: TaskId, t: &Task) -> bool {
    waiting.contains(&id) && t.work.ends_image() && t.thread.exit_mm_done
}

/// Removes `task` from the kernel, counting it out of its thread group if it was still alive.
/// Returns its process when that completed a group stop the task never took its part in (a
/// thread that never started), which the caller reports once it unlocked the task table
/// ([`Kernel::report_group_stop`]).
pub(crate) fn forget_task(state: &mut State, task: TaskId) -> Option<Arc<Process>> {
    let (entry, completed) = unhash_task(state, task)?;
    completed.then_some(entry.process)
}

/// Takes `task` out of the table, its pid and its part in its group's signals and count of
/// live tasks with it. Returns its entry, and whether that completed a group stop.
fn unhash_task(state: &mut State, task: TaskId) -> Option<(Task, bool)> {
    let entry = state.tasks.remove(&task)?;
    // kernel/exit.c `__exit_signal`, under the same lock procfs uses to sum live tasks.
    entry.process.ioac.add(entry.ioac.counts());
    state.pids.detach(task.0, crate::pid::PidType::Pid);
    let completed = entry.signals.detach();
    let group = &entry.process.group;
    if !entry.thread.exited {
        group.state.lock_or_recover().live -= 1;
    }
    group.released.wake(PollMask::empty());
    Some((entry, completed))
}

/// The identity of a released leader until reap, without its execution resources.
/// Linux kernel/sched/syscalls.c:find_process_by_pid still finds its task_struct;
/// kernel/exit.c:release_task/__unhash_process ends that visibility.
pub(crate) struct RetiredTask {
    pub(crate) cred: Arc<crate::cred::Cred>,
    pub(crate) sched: crate::sched::Sched,
}

/// What is left of a task of a reaped process once its reaper took it out of the task table
/// (`release_task`), until the execution layer lets it go (`put_task_struct`): what its
/// instance takes on its way out. Its process, memory, files and the rest went with the reap.
pub(crate) struct DeadTask {
    /// Its exit, or a vfork parent's, which its instance unwinds through.
    pub(crate) work: Work,
    pub(crate) signals: Arc<crate::signal::TaskSignals>,
    pub(crate) cpu: Arc<CpuAccount>,
    /// kernel/fork.c:__put_task_struct releases objective credentials in exit_creds,
    /// after release_task unhashed the task; retain this task's own final snapshot.
    pub(crate) cred: Arc<crate::cred::Cred>,
    /// Its process while anyone else holds it, for [`Kernel::terminate`]'s wait.
    process: std::sync::Weak<Process>,
}
