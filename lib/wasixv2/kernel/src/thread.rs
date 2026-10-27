//! Threads: `clone` for the thread case (through the `thread-spawn` extension), thread groups,
//! `exit` versus `exit_group`, `de_thread` for `execve`, releasing tasks the execution layer is
//! done with, `set_tid_address` and robust futex lists.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    CloneFlags, GuestPtrMut, RobustListHeadPtr, RobustListHeadPtrPtr, S32Ptr, Ulong, Uptr, Usize,
    UsizePtr,
};
use wasixv2_syscalls::{Caller, Errno, GuestAction, SysResult, TaskId, WorkFlag, write_value};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, wait_event};

use crate::{Kernel, Process, State, Task, work::Work};

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
    /// The status of `exit_group`, or of the last task's `exit`.
    exit: Option<i32>,
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
                // A group on its way out starts nothing (Linux: fatal_signal_pending).
                if group.exit.is_some() {
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
                    .attach(id, task.signals.blocked(), flag),
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
        let mut state = self.state.lock().unwrap();
        let unstarted = match state.tasks.get_mut(&task) {
            Some(entry) => entry.work.push(action),
            None => return,
        };
        for child in unstarted {
            forget_task(&mut state, child);
        }
    }

    /// `do_exit` for one task. The last task to exit ends its thread group with its status. The
    /// group leader stays until then, as a zombie leader does on Linux, so the group's status
    /// reaches whoever started the process.
    pub(crate) async fn sys_exit(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let task = caller.task();
        let process = self.process(task);
        let thread = self.take_futex_state(task);
        self.exit_thread(caller, task, thread, true).await;
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
            group.live -= 1;
            group.live == 0
        };
        if last {
            self.mm_release(caller).await;
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

    pub(crate) async fn sys_exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        self.group_exit_futexes(caller).await;
        self.mm_release(caller).await;
        self.do_group_exit(caller.task(), status);
        Ok(())
    }

    /// `do_group_exit`: every task of the group exits with `status`. The first exit wins. Tasks
    /// blocked in an interruptible wait notice; the others unwind on their next return to the
    /// guest or preemption point.
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
        if others {
            signals.kill_all();
        }
        // The dying group's timers stop (`do_exit` with `group_dead`).
        signals.cancel_real_timer();
        crate::posix_timer::exit_itimers(&signals);
        process.group.exited.wake(PollMask::empty());
        // `exit_mm` for what nobody could write back.
        if !self.mm_shared_elsewhere(task) {
            self.mm(task).reset();
        }
        process.group.released.wake(PollMask::empty());
        self.exit_notify(&process);
    }

    /// `exit_notify` once the group has exited and none of its threads is left but the leader
    /// (`thread_group_empty`, `delay_group_leader`): only then the process becomes a zombie,
    /// its files close and its parent hears of it, so nothing of it runs after its exit is seen.
    fn exit_notify(&self, process: &Arc<Process>) {
        let Some(status) = process.group.status() else {
            return;
        };
        let others = {
            let state = self.state.lock().unwrap();
            state
                .tasks
                .iter()
                .any(|(&id, t)| id != process.pid && Arc::ptr_eq(&t.process, process))
        };
        if !others {
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
        let process = {
            let mut state = self.state.lock().unwrap();
            let (unstarted, process) = match state.tasks.get_mut(&task) {
                Some(entry) => (entry.work.take_unstarted(), entry.process.clone()),
                None => return,
            };
            for child in unstarted {
                forget_task(&mut state, child);
            }
            forget_task(&mut state, task);
            process
        };
        self.exit_notify(&process);
    }

    /// `thread-spawn` made `task`, but its thread never started.
    pub(crate) fn abort_thread(&self, task: TaskId) {
        let process = {
            let mut state = self.state.lock().unwrap();
            let Some(process) = state.tasks.get(&task).map(|t| t.process.clone()) else {
                return;
            };
            forget_task(&mut state, task);
            process
        };
        self.exit_notify(&process);
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

    /// `de_thread` at the point of no return of `execve` from `task`: every other task of the
    /// thread group is killed, and the image runs as the group leader, which `task` becomes when
    /// it is not already (Linux exchanges their pids). Returns the leader.
    ///
    /// The leader's new image starts once the others are gone ([`Kernel::exec_wait`]).
    pub(crate) fn de_thread(&self, task: TaskId) -> TaskId {
        let mut state = self.state.lock().unwrap();
        let process = state.tasks[&task].process.clone();
        let leader = process.pid;
        let mut unstarted = Vec::new();
        let mut killed = Vec::new();
        for (&id, t) in state.tasks.iter_mut() {
            if !Arc::ptr_eq(&t.process, &process) || id == leader {
                continue;
            }
            t.thread.exited = true;
            unstarted.extend(t.work.push(GuestAction::Exit(0)));
            if id != task {
                killed.push(t.signals.clone());
            }
        }
        let group = state.tasks[&leader].signals.group().clone();
        if task != leader {
            // The leader takes over the caller's signal state; what it was doing ends here.
            group.take_over(task, leader);
            let entry = state.tasks.get_mut(&leader).expect("the leader is listed");
            unstarted.extend(entry.work.take_unstarted());
            entry.thread.exited = false;
            entry.restart = None;
        }
        for child in unstarted {
            forget_task(&mut state, child);
        }
        process.group.state.lock().unwrap().live = 1;
        drop(state);
        // `zap_other_threads`: SIGKILL for each, so blocked ones wake up.
        for signals in killed {
            signals.kill();
        }
        leader
    }

    /// Leaves the image of an `execve` for the leader `task`, and wakes it wherever it waits,
    /// a zombie leader's wait for its group included.
    pub(crate) fn hand_image(&self, task: TaskId, vfork_parent: Option<TaskId>) {
        self.queue(task, GuestAction::Exec { vfork_parent });
        let (process, signals) = self.with_task(task, |t| (t.process.clone(), t.signals.clone()));
        signals.group().notify(task);
        process.group.exited.wake(PollMask::empty());
    }

    /// Waits until `task` is the only task of its thread group, before the image `execve` loaded
    /// starts. `Err(status)` when the group exited meanwhile; its exit is taken.
    pub(crate) async fn exec_wait(&self, task: TaskId) -> Result<(), i32> {
        let process = self.process(task);
        let alone = || {
            let mut state = self.state.lock().unwrap();
            let Some(entry) = state.tasks.get_mut(&task) else {
                return Some(Err(0));
            };
            if let Some(status) = entry.work.exit() {
                while entry.work.take().is_some() {}
                return Some(Err(status));
            }
            let others = state
                .tasks
                .iter()
                .any(|(&id, t)| id != task && Arc::ptr_eq(&t.process, &process));
            (!others).then_some(Ok(()))
        };
        wait_event(&process.group.released, alone, &NoInterrupt)
            .await
            .expect("NoInterrupt never interrupts")
    }

    /// The futex state `task` leaves behind, which its exit releases once: Linux clears
    /// `robust_list` and `clear_child_tid` as it handles them.
    fn take_futex_state(&self, task: TaskId) -> ThreadState {
        self.with_task(task, |t| {
            let thread = t.thread;
            (t.thread.robust_list, t.thread.clear_child_tid) = (0, 0);
            thread
        })
    }

    /// `futex_exit_release` and `mm_release` for every task of the caller's thread group, run
    /// by the caller while the memory is still there. `exit_group` and `execve` end the group's
    /// other tasks (`zap_other_threads`, `de_thread`), and the end of the memory, which writes
    /// back its shared mappings, comes before those tasks unwind; so their robust and PI futexes
    /// go to their waiters here, as each task's own `do_exit` would. A `clear_child_tid` is only
    /// written when the memory outlives the group (a vfork relative), as Linux writes it only
    /// while others use the memory.
    pub(crate) async fn group_exit_futexes(&self, caller: &Caller<'_>) {
        let tasks: Vec<TaskId> = {
            let state = self.state.lock().unwrap();
            let process = &state.tasks[&caller.task()].process;
            state
                .tasks
                .iter()
                .filter(|(_, t)| Arc::ptr_eq(&t.process, process) && !t.thread.exited)
                .map(|(&id, _)| id)
                .collect()
        };
        let clear_tid = self.mm_shared_elsewhere(caller.task());
        for task in tasks {
            let thread = self.take_futex_state(task);
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
                let key = crate::futex::Key {
                    mm: self.mm(caller.task()).id(),
                    addr: thread.clear_child_tid,
                };
                let _ = self.futexes.wake(key, 1, u32::MAX);
            }
        }
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

/// Removes `task` from the kernel, counting it out of its thread group if it was still alive.
pub(crate) fn forget_task(state: &mut State, task: TaskId) {
    let Some(entry) = state.tasks.remove(&task) else {
        return;
    };
    entry.signals.detach();
    let group = &entry.process.group;
    if !entry.thread.exited {
        group.state.lock().unwrap().live -= 1;
    }
    group.released.wake(PollMask::empty());
}
