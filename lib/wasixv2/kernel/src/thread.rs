//! Threads: `clone` for the thread case (through the `thread-spawn` extension), thread groups,
//! `exit` versus `exit_group`, `de_thread` for `execve`, releasing tasks the execution layer is
//! done with, `set_tid_address` and robust futex lists.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    CloneFlags, GuestPtrMut, RobustListHeadPtr, RobustListHeadPtrPtr, S32Ptr, SiCode, Ulong, Uptr,
    Usize, UsizePtr,
};
use wasixv2_syscalls::{
    Caller, Errno, GuestAction, Image, Resume, SysResult, TaskId, WorkFlag, write_value,
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
        group.state.lock().unwrap().live = 1;
        group
    }

    /// The group is exiting or exited.
    pub(crate) fn exiting(&self) -> bool {
        self.state.lock().unwrap().exit.is_some()
    }

    /// `signal_group_exit`: the group is exiting, or one of its tasks is in an `execve` that
    /// is ending the others. It starts no new tasks.
    pub(crate) fn signal_group_exit(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.exit.is_some() || state.exec.is_some()
    }

    fn status(&self) -> Option<i32> {
        self.state.lock().unwrap().exit
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
    /// The task called `exit` or was killed with its thread; it goes away once the execution
    /// layer releases it.
    exited: bool,
    /// The task is done with its memory ([`Kernel::exit_mm`]; Linux clears `tsk->mm`), so it
    /// no longer counts among the memory's users.
    left_mm: bool,
    /// The task died while it waited for its vfork child, and its `exit_mm` went to the
    /// instance the child runs in ([`GuestAction::ExitMm`]); it goes there once.
    exit_mm_handed: bool,
}

impl ThreadState {
    /// The task called `exit` or was killed with its thread.
    pub(crate) fn exited(&self) -> bool {
        self.exited
    }

    /// The thread state of a new process's first task, made from the caller's (`copy_process`):
    /// no robust list, the caller's thread pointer unless `CLONE_SETTLS` gives `tls`, and
    /// `CLONE_CHILD_CLEARTID`'s `child_tid`.
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
            exited: false,
            left_mm: false,
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
    /// group, writes its tid where asked, and leaves the start to the execution layer.
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
            let mut state = self.state.lock().unwrap();
            let task = state
                .tasks
                .get(&parent)
                .expect("syscall from an unknown task");
            let process = task.process.clone();
            if crate::resource::nproc_exceeded(&state, &process) {
                return Err(Errno::EAGAIN.into());
            }
            let sched = task.sched.for_child()?;
            {
                let mut group = process.group.state.lock().unwrap();
                // A group on its way out, or in an `execve`, starts nothing (Linux:
                // `fatal_signal_pending`, the leader's SIGKILL from `de_thread`).
                if group.exit.is_some() || group.exec.is_some() {
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
                exited: false,
                left_mm: false,
                exit_mm_handed: false,
            };
            let id = TaskId(state.last_id + 1);
            let flag = Arc::new(WorkFlag::default());
            let new = Task {
                process,
                mm: task.mm.clone(),
                files: task.files.clone(),
                fs: task.fs.clone(),
                work: Work::new(flag.clone()),
                restart: None,
                comm: task.comm,
                sched,
                thread,
                signals: task
                    .signals
                    .group()
                    .attach(id, task.signals.inherit(THREAD), flag),
            };
            state.last_id += 1;
            state.tasks.insert(id, new);
            id
        };
        let tid = child.0 as i32;
        // Linux ignores faults on both writes.
        if flags.contains(CloneFlags::CLONE_PARENT_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(parent_tid.addr()), &tid).await;
        }
        if flags.contains(CloneFlags::CLONE_CHILD_SETTID) {
            let _ = write_value(caller.memory(), GuestPtrMut::new(child_tid.addr()), &tid).await;
        }
        let start = GuestAction::StartThread {
            task: child,
            start_arg,
        };
        self.queue(parent, start);
        Ok(tid)
    }

    /// Leaves `action` for the execution layer on `task`'s next return to the guest.
    pub(crate) fn queue(&self, task: TaskId, action: GuestAction) {
        let unstarted = {
            let mut state = self.state.lock().unwrap();
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
            let state = self.state.read().unwrap();
            match state.tasks.get(&child) {
                Some(entry) => entry.process.clone(),
                None => return,
            }
        };
        if process.pid == child {
            self.exit_files(&process);
            let vfork_parent = {
                let mut tree = self.procs.tree.lock().unwrap();
                tree.nodes.remove(&child.0);
                tree.vforks.remove(&child)
            };
            if let Some(parent) = vfork_parent {
                self.vfork_done(parent);
            }
        }
        let stopped = forget_task(&mut self.state.lock().unwrap(), child);
        self.report_group_stop(stopped);
    }

    /// `do_exit` for one task: its memory goes first ([`Kernel::exit_mm`]). The last task to
    /// exit ends its thread group, with the leader's status. The group leader stays until then,
    /// as a zombie leader does on Linux, so the group's status reaches whoever started the
    /// process.
    pub(crate) async fn sys_exit(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let task = caller.task();
        let process = self.process(task);
        self.exit_mm(caller).await;
        let last = {
            let mut state = self.state.lock().unwrap();
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
            let mut group = process.group.state.lock().unwrap();
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
        let mut state = self.state.lock().unwrap();
        let process = state.tasks[&task].process.clone();
        let status = *process
            .group
            .state
            .lock()
            .unwrap()
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
    fn exit_notify(&self, process: &Arc<Process>) {
        let Some(status) = process.group.status() else {
            return;
        };
        let waiting = self.vfork_parents();
        let vfork_child = self.vfork_child(process.pid);
        let (others, leader_runs) = {
            let state = self.state.lock().unwrap();
            let gone = |id: &TaskId, t: &Task| gone_in_vfork(&waiting, *id, t);
            let others = state.tasks.iter().any(|(id, t)| {
                *id != process.pid && Arc::ptr_eq(&t.process, process) && !gone(id, t)
            });
            let leader_runs = state.tasks.get(&process.pid).is_some_and(|t| {
                if waiting.contains(&process.pid) {
                    !gone(&process.pid, t)
                } else {
                    Arc::ptr_eq(&t.process, process) && !t.thread.left_mm && !vfork_child
                }
            });
            (others, leader_runs)
        };
        if !others && !leader_runs {
            // ---- ipc ----
            // `exit_sem`, by the group's last task.
            self.ipc_process_exited(process.pid.0);
            // ---- proc ----
            self.process_exited(process, process.pid, status);
        }
    }

    /// `release_task`: the execution layer is done with `task`, so it goes. A zombie process
    /// lives on in the process tree.
    pub(crate) fn release_task(&self, task: TaskId) {
        let (process, stopped) = {
            let mut state = self.state.lock().unwrap();
            let (unstarted, process) = match state.tasks.get_mut(&task) {
                Some(entry) => (entry.work.take_unstarted(), entry.process.clone()),
                None => return,
            };
            let mut stopped = None;
            for child in unstarted {
                stopped = stopped.or(forget_task(&mut state, child));
            }
            stopped = stopped.or(forget_task(&mut state, task));
            (process, stopped)
        };
        self.report_group_stop(stopped);
        self.exit_notify(&process);
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
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .tasks
            .values()
            .find(|t| Arc::ptr_eq(&t.process, process))
            .map(|t| t.signals.group().clone())
    }

    /// `thread-spawn` made `task`, but its thread never started.
    pub(crate) fn abort_thread(&self, task: TaskId) {
        let (process, stopped) = {
            let mut state = self.state.lock().unwrap();
            let Some(process) = state.tasks.get(&task).map(|t| t.process.clone()) else {
                return;
            };
            (process, forget_task(&mut state, task))
        };
        self.report_group_stop(stopped);
        self.exit_notify(&process);
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
            let processes = self.live_processes();
            if processes.is_empty() {
                return;
            }
            for (_, signals) in &processes {
                signals.kill_all();
            }
            for (process, _) in processes {
                let gone = || {
                    let state = self.state.lock().unwrap();
                    let left = state
                        .tasks
                        .values()
                        .any(|t| Arc::ptr_eq(&t.process, &process));
                    (!left).then_some(())
                };
                wait_event(&process.group.released, gone, &NoInterrupt)
                    .await
                    .expect("NoInterrupt never interrupts");
            }
        }
    }

    /// Every process that has a task, with its thread group's signals; not the kernel's init,
    /// which has no instance to end.
    fn live_processes(&self) -> Vec<(Arc<Process>, Arc<Signals>)> {
        let state = self.state.lock().unwrap();
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
        let process = self.process(task);
        let alone = || {
            let state = self.state.lock().unwrap();
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
        let mut state = self.state.lock().unwrap();
        let process = state.tasks[&task].process.clone();
        let leader = process.pid;
        {
            let mut group = process.group.state.lock().unwrap();
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
        process.group.state.lock().unwrap().live -= ended;
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
            let state = self.state.read().unwrap();
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
                .is_some_and(|t| t.thread.left_mm && !t.work.exec_resumes_vfork_parent());
            let dead_in_vfork = left && self.vfork_waits(leader);
            let group = process.group.state.lock().unwrap();
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
            let entry = state.tasks.get_mut(&leader).expect("the leader is listed");
            if detached {
                entry.work.detach_exec();
            }
            unstarted.extend(entry.work.take_unstarted());
            entry.thread.exited = false;
            entry.restart = None;
        }
        for child in unstarted {
            forget_task(&mut state, child);
        }
        process.group.state.lock().unwrap().live = 1;
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
            self.vfork_done(parent.task);
        }
        let (process, signals) = {
            let mut state = self.state.lock().unwrap();
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
        if let Some(exec) = &mut process.group.state.lock().unwrap().exec {
            exec.leader_parked = true;
        }
        process.group.released.wake(PollMask::empty());
        let alone = || {
            let waiting = self.vfork_parents();
            let mut state = self.state.lock().unwrap();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return Some(Err(0));
            };
            if let Some(status) = entry.work.exit() {
                while entry.work.take().is_some() {}
                return Some(Err(status));
            }
            if process.group.state.lock().unwrap().exec.is_some() {
                return None;
            }
            let others = state.tasks.iter().any(|(&id, t)| {
                id != task && Arc::ptr_eq(&t.process, &process) && !gone_in_vfork(&waiting, id, t)
            });
            (!others).then_some(Ok(()))
        };
        wait_event(&process.group.released, alone, &NoInterrupt)
            .await
            .expect("NoInterrupt never interrupts")
    }

    /// The futex state of every task of `task`'s thread group that has not exited. Taken all
    /// at once: a task that exits meanwhile can be released, and gone, before the caller
    /// reaches it.
    pub(crate) fn take_group_futex_states(&self, task: TaskId) -> Vec<(TaskId, ThreadState)> {
        let mut state = self.state.lock().unwrap();
        let process = state.tasks[&task].process.clone();
        state
            .tasks
            .iter_mut()
            .filter(|(_, t)| Arc::ptr_eq(&t.process, &process) && !t.thread.exited)
            .map(|(&id, t)| (id, take_futex_state(t)))
            .collect()
    }

    /// `exec_mm_release` for the futex state of `threads`, tasks of the caller's group that
    /// stopped running the old image: their robust and PI futexes go to their waiters, and a
    /// `clear_child_tid` is only written when the memory outlives the group (a vfork relative),
    /// as Linux writes it only while others use the memory.
    pub(crate) async fn exit_futexes(
        &self,
        caller: &Caller<'_>,
        threads: Vec<(TaskId, ThreadState)>,
    ) {
        let clear_tid = self.mm_shared_elsewhere(caller.task());
        for (task, thread) in threads {
            self.exit_thread(caller, task, thread, clear_tid).await;
        }
    }

    /// `futex_exit_release` and `mm_release` for `task`, on the memory it runs on: releases
    /// the robust futexes it holds and hands its PI futexes on, then, with `clear_tid`, zeroes
    /// its `clear_child_tid` and wakes a waiter there.
    async fn exit_thread(
        &self,
        caller: &Caller<'_>,
        task: TaskId,
        thread: ThreadState,
        clear_tid: bool,
    ) {
        let tid = task.0;
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
    /// robust and PI futexes and, while another task still uses the memory (`mm_users > 1`: a
    /// thread of its group, or a vfork relative), zeroes its `clear_child_tid` and wakes a
    /// waiter there; then `mmput`, where the last task to leave the memory writes back its
    /// shared mappings and drops them (`exit_mmap`). Once per task and memory; a later call does
    /// nothing.
    pub(crate) async fn exit_mm(&self, caller: &Caller<'_>) {
        let task = caller.task();
        let (thread, users) = {
            let mut state = self.state.lock().unwrap();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return;
            };
            if entry.thread.left_mm {
                return;
            }
            let thread = take_futex_state(entry);
            (thread, mm_users(&state, task))
        };
        self.exit_thread(caller, task, thread, users > 1).await;
        let (last, process) = {
            let mut state = self.state.lock().unwrap();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return;
            };
            entry.thread.left_mm = true;
            let process = entry.process.clone();
            (mm_users(&state, task) == 0, process)
        };
        if last {
            self.exit_mmap(caller).await;
        }
        // A leader whose group exited was all its exit waited for, and so was a task that
        // died waiting for its vfork child, whose instance runs on.
        if task == process.pid || self.vfork_waits(task) {
            self.exit_notify(&process);
            process.group.released.wake(PollMask::empty());
        }
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
        caller.task().0 as i32
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
        let head = {
            let state = self.state.lock().unwrap();
            let me = &state.tasks[&caller.task()];
            let target = match pid {
                0 => Some(me),
                pid => state
                    .tasks
                    .get(&TaskId(pid as u32))
                    .filter(|t| !t.thread.exited),
            };
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
        group.state.lock().unwrap().exec = None;
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
    waiting.contains(&id) && t.work.ends_image() && t.thread.left_mm
}

/// Removes `task` from the kernel, counting it out of its thread group if it was still alive.
/// Returns its process when that completed a group stop the task never took its part in (a
/// thread that never started), which the caller reports once it unlocked the task table
/// ([`Kernel::report_group_stop`]).
pub(crate) fn forget_task(state: &mut State, task: TaskId) -> Option<Arc<Process>> {
    let entry = state.tasks.remove(&task)?;
    let completed = entry.signals.detach();
    let group = &entry.process.group;
    if !entry.thread.exited {
        group.state.lock().unwrap().live -= 1;
    }
    group.released.wake(PollMask::empty());
    completed.then_some(entry.process)
}
