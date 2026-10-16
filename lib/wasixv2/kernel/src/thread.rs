//! Threads: `clone` for the thread case (through the `thread-spawn` extension), thread groups,
//! `exit` versus `exit_group`, `set_tid_address` and robust futex lists.

use std::sync::{Arc, Mutex};

use wasixv2_abi::{
    CloneFlags, GuestPtrMut, RobustListHeadPtr, RobustListHeadPtrPtr, S32Ptr, Ulong, Uptr, Usize,
    UsizePtr,
};
use wasixv2_syscalls::{Caller, Errno, GuestAction, SysResult, TaskId, write_value};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, wait_event};

use crate::{Kernel, Task};

/// `sizeof(struct robust_list_head)` on ILP32.
const ROBUST_LIST_HEAD_SIZE: u32 = 12;

/// A thread group (a process, in user terms): which of its tasks are still running, and how it
/// ended.
#[derive(Default)]
pub(crate) struct Group {
    state: Mutex<GroupState>,
    /// Woken once the group exits, for its tasks blocked in the kernel.
    pub(crate) exited: WaitQueue,
}

#[derive(Default)]
struct GroupState {
    /// Tasks that have not called `exit` yet.
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
    /// The task called `exit`; it goes away once the execution layer saw that.
    exited: bool,
}

impl ThreadState {
    /// Whether the task ended and its last pending action was taken, so it can be forgotten.
    pub(crate) fn reapable(&self, pending: Option<GuestAction>) -> bool {
        self.exited && matches!(pending, Some(GuestAction::Exit(_)))
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
            // Every task belongs to the one user.
            if process
                .resources
                .lock()
                .unwrap()
                .nproc_reached(state.tasks.len())
            {
                return Err(Errno::EAGAIN.into());
            }
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
            let new = Task {
                process,
                files: task.files.clone(),
                fs: task.fs.clone(),
                pending: None,
                thread,
                signals: task.signals.group().attach(id, task.signals.blocked()),
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
        self.with_task(parent, |task| task.pending = Some(start));
        Ok(tid)
    }

    /// `do_exit` for one task. The last task to exit ends its thread group with its status. The
    /// group leader stays until then, as a zombie leader does on Linux, so the group's status
    /// reaches whoever started the process.
    pub(crate) async fn sys_exit(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        let task = caller.task();
        let (process, thread) = self.with_task(task, |t| (t.process.clone(), t.thread));
        self.exit_thread(caller, thread).await;
        let last = {
            let mut group = process.group.state.lock().unwrap();
            group.live -= 1;
            group.live == 0
        };
        if last {
            self.do_group_exit(task, status);
            return Ok(());
        }
        if task == process.pid {
            let status = wait_event(
                &process.group.exited,
                || process.group.status(),
                &NoInterrupt,
            )
            .await
            .expect("NoInterrupt never interrupts");
            self.with_task(task, |t| t.pending = Some(GuestAction::Exit(status)));
            return Ok(());
        }
        self.with_task(task, |t| {
            t.thread.exited = true;
            t.pending = Some(GuestAction::Exit(status & 0xff));
        });
        Ok(())
    }

    pub(crate) async fn sys_exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        self.do_group_exit(caller.task(), status);
        Ok(())
    }

    /// `do_group_exit`: every task of the group exits with `status`. The first exit wins. Tasks
    /// blocked in an interruptible wait notice; the others unwind on their next return to the guest.
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
        for (&id, t) in state.tasks.iter_mut() {
            if Arc::ptr_eq(&t.process, &process) {
                t.pending = Some(GuestAction::Exit(status));
                others |= id != task;
            }
        }
        let signals = state.tasks[&task].signals.group().clone();
        drop(state);
        if others {
            signals.kill_all();
        }
        process.group.exited.wake(PollMask::empty());
        // ---- proc ----
        self.process_exited(&process, task, status);
    }

    /// `mm_release` and `exit_robust_list`: what a task's exit does to the memory it shared.
    async fn exit_thread(&self, caller: &Caller<'_>, thread: ThreadState) {
        let tid = caller.task().0;
        if thread.robust_list != 0 {
            self.exit_robust_list(caller, thread.robust_list, tid).await;
        }
        if thread.clear_child_tid != 0 {
            let ptr = GuestPtrMut::<u32>::new(thread.clear_child_tid);
            if write_value(caller.memory(), ptr, &0).await.is_ok() {
                let key = crate::futex::Key {
                    mm: self.process(caller.task()).pid.0,
                    addr: thread.clear_child_tid,
                };
                self.futexes.wake(key, 1, u32::MAX);
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
