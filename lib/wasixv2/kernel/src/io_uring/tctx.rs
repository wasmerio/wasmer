//! A task's io_uring context (`io_uring/tctx.c`, `cancel.c`): the rings it submitted to, whose
//! requests its exit and `execve` cancel (`io_uring_files_cancel`, `io_uring_task_cancel`),
//! and the ring descriptors it registered (`IORING_REGISTER_RING_FDS`).
//!
//! A request that holds a ring's file, as a poll of the ring itself does, keeps that ring
//! alive however its descriptors close; the exit of its submitter cancels it, which lets the
//! ring go, as Linux tracks such requests (`REQ_F_INFLIGHT`) for `io_uring_files_cancel`.

use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

use wasixv2_abi::{GuestLayout, GuestPtr, GuestPtrMut, IoUringRsrcUpdate};
use wasixv2_syscalls::{Caller, Errno, Fd, SysResult, TaskId, read_value, write_value};

use super::{Ctx, is_ring, ops::Req};
use crate::{Kernel, file::OpenFile, sync::MutexExt};

/// `IO_RINGFD_REG_MAX`.
pub(crate) const RINGFD_REG_MAX: u32 = 16;

/// `struct io_uring_task`.
#[derive(Default)]
pub(crate) struct Tctx {
    state: Mutex<TctxState>,
    pending: AtomicBool,
}

/// io_uring/tw.c:io_req_task_work_add_remote: a task's notification target.
pub(super) struct TaskWork {
    tctx: Arc<Tctx>,
    signals: Arc<crate::signal::TaskSignals>,
    task: TaskId,
}

impl TaskWork {
    pub(super) fn notify(&self) {
        self.tctx.pending.store(true, Ordering::Release);
        // sched/signal.h:__set_notify_signal wakes TASK_INTERRUPTIBLE even for
        // COOP's TWA_SIGNAL_NO_IPI. A running guest observes WorkFlag at its
        // existing preemption/return points; there is no host reschedule IPI here.
        self.signals.group().notify_uring(self.task);
    }
}

#[derive(Default)]
struct TctxState {
    /// The rings the task submitted to (`tctx->xa`).
    rings: Vec<Weak<Ctx>>,
    /// `registered_rings`.
    registered: [Option<Arc<OpenFile>>; RINGFD_REG_MAX as usize],
}

impl Tctx {
    pub(crate) fn pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    /// `__io_uring_add_tctx_node`: the task uses `ctx`.
    pub(crate) fn add(&self, ctx: &Arc<Ctx>) {
        let mut state = self.state.lock_or_recover();
        state.rings.retain(|ring| ring.strong_count() > 0);
        if !state.rings.iter().any(|ring| ring.ptr_eq(&ctx.me)) {
            state.rings.push(ctx.me.clone());
        }
    }

    fn rings(&self) -> Vec<Arc<Ctx>> {
        let state = self.state.lock_or_recover();
        state.rings.iter().filter_map(Weak::upgrade).collect()
    }

    /// The ring registered at `index` (`io_uring_ctx_get_file`'s registered case).
    pub(crate) fn registered(&self, index: u32) -> Result<Arc<OpenFile>, Errno> {
        if index >= RINGFD_REG_MAX {
            return Err(Errno::EINVAL);
        }
        let state = self.state.lock_or_recover();
        state.registered[index as usize].clone().ok_or(Errno::EBADF)
    }

    /// `io_ring_add_registered_file`: the first free slot of `[start, end)`.
    pub(super) fn add_registered(
        &self,
        file: Arc<OpenFile>,
        start: u32,
        end: u32,
    ) -> Result<u32, Errno> {
        let mut state = self.state.lock_or_recover();
        let slot = (start..end).find(|&i| state.registered[i as usize].is_none());
        let slot = slot.ok_or(Errno::EBUSY)?;
        state.registered[slot as usize] = Some(file);
        Ok(slot)
    }

    fn remove_registered(&self, index: u32) -> Option<Arc<OpenFile>> {
        self.state.lock_or_recover().registered[index as usize].take()
    }

    /// `io_uring_unreg_ringfd`.
    fn unregister_all(&self) -> Vec<Arc<OpenFile>> {
        let mut state = self.state.lock_or_recover();
        state
            .registered
            .iter_mut()
            .filter_map(Option::take)
            .collect()
    }
}

/// Which of a task's requests a cancellation takes (`io_uring_try_cancel_requests`' flags).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cancel {
    /// `execve`: every request (`IO_CANCEL_ALL`).
    All,
    /// The exit of the whole thread group: every request but timeouts, which nobody will
    /// look at anyway (`IO_CANCEL_KEEP_TIMEOUTS`).
    GroupExit,
    /// The exit of one thread: the requests that hold a ring (`REQ_F_INFLIGHT`).
    Tracked,
}

impl Cancel {
    pub(super) fn matches(self, req: &Req) -> bool {
        match self {
            Cancel::All => true,
            Cancel::GroupExit => !req.is_timeout(),
            Cancel::Tracked => req.holds_ring(),
        }
    }
}

impl Kernel {
    pub(super) fn uring_task_work(&self, task: TaskId) -> Arc<TaskWork> {
        Arc::new(TaskWork {
            tctx: self.tctx(task),
            signals: self.task_signals(task),
            task,
        })
    }

    /// io_uring/tw.c:tctx_task_work_run, called before get_signal's task_work_run boundary.
    pub(crate) fn io_uring_run_task_work<'a>(
        &'a self,
        caller: &'a Caller<'_>,
    ) -> wasixv2_syscalls::LocalFuture<'a, ()> {
        let context = self
            .state
            .read_or_recover()
            .tasks
            .get(&caller.task())
            .filter(|task| task.uring.pending())
            .map(|task| (task.uring.clone(), task.mm.id(), task.signals.clone()));
        let Some((tctx, mm, signals)) = context else {
            // A capture-free PollFn is zero-sized: boxing this completed future
            // allocates nothing on signal returns of tasks without io_uring work.
            return Box::pin(std::future::poll_fn(|_| std::task::Poll::Ready(())));
        };
        Box::pin(async move {
            while tctx.pending.swap(false, Ordering::AcqRel) {
                signals.group().clear_uring_notify(caller.task());
                for ctx in tctx.rings() {
                    if ctx
                        .shape
                        .has(wasixv2_abi::IoringSetupFlags::IORING_SETUP_DEFER_TASKRUN)
                    {
                        continue;
                    }
                    // io_uring/tw.c:tctx_task_work_run issues under uring_lock.
                    // This cooperative return hook leaves busy work notified
                    // for the next boundary/enter instead of parking its executor
                    // behind another task's long buffer registration.
                    let Some(_submit) = ctx.submit.try_lock() else {
                        tctx.pending.store(true, Ordering::Release);
                        signals.group().notify_uring(caller.task());
                        return;
                    };
                    self.run_task_work(caller, &ctx, mm).await;
                }
            }
        })
    }

    pub(crate) fn tctx(&self, task: TaskId) -> Arc<Tctx> {
        self.task_ref(task, |t| t.uring.clone())
    }

    /// `__io_uring_cancel`: the task's registered rings go, and its requests on every ring
    /// it used, as `how` says, complete with `ECANCELED`.
    pub(crate) fn io_uring_cancel(&self, task: TaskId, how: Cancel) {
        let tctx = self.tctx(task);
        drop(tctx.unregister_all());
        for ctx in tctx.rings() {
            ctx.cancel_task(task, how);
        }
    }

    /// `io_uring_files_cancel` in `do_exit`: a thread's exit cancels what holds a ring, the
    /// group's every request but its timeouts.
    pub(crate) fn io_uring_files_cancel(&self, task: TaskId) {
        let group_exit = self.process(task).group.status().is_some();
        let how = match group_exit {
            true => Cancel::GroupExit,
            false => Cancel::Tracked,
        };
        self.io_uring_cancel(task, how);
    }

    /// `io_ringfd_register`: registers the rings each `io_uring_rsrc_update` of `arg` names by
    /// descriptor, at its offset or, for -1, the first free one, which is written back.
    /// Returns how many were registered, or the error of the first.
    pub(super) async fn ringfd_register(
        &self,
        caller: &Caller<'_>,
        ctx: &Arc<Ctx>,
        arg: u32,
        nr_args: u32,
    ) -> SysResult<i32> {
        if nr_args == 0 || nr_args > RINGFD_REG_MAX {
            return Err(Errno::EINVAL.into());
        }
        let tctx = self.tctx(caller.task());
        tctx.add(ctx);
        let mut done = 0;
        let mut err = Errno::EINVAL;
        for i in 0..nr_args {
            let at = arg + i * IoUringRsrcUpdate::SIZE as u32;
            let Ok(mut reg) =
                read_value::<IoUringRsrcUpdate>(caller.memory(), GuestPtr::new(at)).await
            else {
                err = Errno::EFAULT;
                break;
            };
            if reg.resv != 0 {
                break;
            }
            let (start, end) = match reg.offset {
                u32::MAX => (0, RINGFD_REG_MAX),
                offset if offset >= RINGFD_REG_MAX => break,
                offset => (offset, offset + 1),
            };
            let file = match self.ring_file(caller, reg.data) {
                Ok(file) => file,
                Err(errno) => {
                    err = errno;
                    break;
                }
            };
            reg.offset = match tctx.add_registered(file, start, end) {
                Ok(slot) => slot,
                Err(errno) => {
                    err = errno;
                    break;
                }
            };
            if write_value(caller.memory(), GuestPtrMut::new(at), &reg)
                .await
                .is_err()
            {
                drop(tctx.remove_registered(reg.offset));
                err = Errno::EFAULT;
                break;
            }
            done += 1;
        }
        match done {
            0 => Err(err.into()),
            done => Ok(done),
        }
    }

    /// The ring open as `fd`: `EBADF` unless open, `EOPNOTSUPP` unless a ring.
    fn ring_file(&self, caller: &Caller<'_>, fd: u64) -> Result<Arc<OpenFile>, Errno> {
        let fd = i32::try_from(fd).map_err(|_| Errno::EBADF)?;
        let file = self.file(caller.task(), Fd(fd))?;
        match is_ring(&file) {
            true => Ok(file),
            false => Err(Errno::EOPNOTSUPP),
        }
    }

    /// `io_ringfd_unregister`: the slots each `io_uring_rsrc_update` of `arg` names.
    pub(super) async fn ringfd_unregister(
        &self,
        caller: &Caller<'_>,
        arg: u32,
        nr_args: u32,
    ) -> SysResult<i32> {
        if nr_args == 0 || nr_args > RINGFD_REG_MAX {
            return Err(Errno::EINVAL.into());
        }
        let tctx = self.tctx(caller.task());
        let mut done = 0;
        let mut err = Errno::EINVAL;
        for i in 0..nr_args {
            let at = arg + i * IoUringRsrcUpdate::SIZE as u32;
            let Ok(reg) = read_value::<IoUringRsrcUpdate>(caller.memory(), GuestPtr::new(at)).await
            else {
                err = Errno::EFAULT;
                break;
            };
            if reg.resv != 0 || reg.data != 0 || reg.offset >= RINGFD_REG_MAX {
                break;
            }
            drop(tctx.remove_registered(reg.offset));
            done += 1;
        }
        match done {
            0 => Err(err.into()),
            done => Ok(done),
        }
    }

    /// `io_uring_ctx_get_file`: the ring `fd` names, a descriptor or, `registered`, a slot of
    /// the caller's registered rings.
    pub(super) fn ring_ctx_file(
        &self,
        caller: &Caller<'_>,
        fd: Fd,
        registered: bool,
    ) -> Result<Arc<OpenFile>, Errno> {
        let file = match registered {
            true => self.tctx(caller.task()).registered(fd.0 as u32)?,
            false => self.file(caller.task(), fd)?,
        };
        match is_ring(&file) {
            true => Ok(file),
            false => Err(Errno::EOPNOTSUPP),
        }
    }
}
