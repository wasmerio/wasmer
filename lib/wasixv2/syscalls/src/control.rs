use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{Errno, Siginfo};

use crate::{
    Layered, SysResult,
    caller::{Caller, TaskId},
    memory::LocalFuture,
};

/// Work the execution layer must do before control returns to the guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestAction {
    /// The task ended: its thread group exited with this status, or, for a task that is not the
    /// group leader, the task itself did.
    Exit(i32),
    /// The calling task created the thread `task` (`thread-spawn`); start it on its own instance
    /// at the module's thread entry point with `start_arg`.
    StartThread { task: TaskId, start_arg: u32 },
    /// Run a signal handler on top of the interrupted guest stack, then report back through
    /// [`Control::signal_return`].
    Signal(SignalFrame),
    // ---- proc ----
    /// The task replaced its image (`execve`): run the module [`Control::take_image`] hands
    /// out as the same task, in a new instance with new memory, from `_start`. With
    /// `vfork_parent`, the task was a vfork child running in its parent's instance: the new
    /// instance runs on its own, and this one goes back to the parent as for
    /// [`GuestAction::VforkReturn`].
    Exec { vfork_parent: Option<VforkParent> },
    /// The instance runs as `task` from now on, and the current call returns `ret`: `clone`
    /// returning 0 to a vfork child.
    SwitchTask { task: TaskId, ret: i32 },
    /// The calling task created the process `child` with a copy of its address space (`clone`
    /// without `CLONE_VM`, which only an address space that
    /// [`can_duplicate`](crate::UserMemory::can_duplicate) gets): copy the memory and the guest's
    /// call stack to a new instance that runs as `child`, where the call returns 0, while here it
    /// returns the child's pid. clone's writes land after the copy, each in one address space:
    /// the child's tid at `parent_tid` in this one (`CLONE_PARENT_SETTID`), and at `child_tid`
    /// in the child's (`CLONE_CHILD_SETTID`), whose thread pointer becomes `tls`
    /// (`CLONE_SETTLS`).
    Fork {
        child: TaskId,
        parent_tid: Option<u32>,
        child_tid: Option<u32>,
        tls: Option<u32>,
    },
    /// The vfork child `child`, running in its parent's instance, ended: the instance runs as
    /// `parent` again, and the guest's `wasixv2_vfork_return(child)` export unwinds to the
    /// vfork point, unless the parent's exit is pending.
    VforkReturn { parent: VforkParent, child: TaskId },
}

/// The vfork parent an instance goes back to once the child running in it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VforkParent {
    pub task: TaskId,
    /// The parent's pending exit, which [`Control::take_pending`] fills in when it hands out
    /// the action: its thread group exited while it waited for the child. Such a parent never
    /// returns from vfork, as Linux kills it in `wait_for_vfork_done`: the instance unwinds it
    /// through its exit with this status instead of resuming it.
    pub exit: Option<i32>,
}

/// One signal handler invocation: what the execution layer writes into the guest's signal frame
/// and which guest function it calls (see `signal-callback-register` in the ABI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalFrame {
    /// The registered trampoline, an index into the guest's function table.
    pub trampoline: u32,
    pub info: Siginfo,
    /// `sa_handler`, an index into the guest's function table.
    pub handler: u32,
    /// `sa_flags`.
    pub flags: u32,
    /// The mask in force before delivery, which the guest finds in `uc_sigmask`; bit `n - 1` is
    /// signal `n`.
    pub mask: u64,
    /// The task's alternate stack as delivery found it, which the guest finds in `uc_stack`
    /// (`__save_altstack`): `ss_sp`, `ss_flags` as `sigaltstack` set them, `ss_size`. The frame
    /// goes on it as [`SignalFrame::altstack_top`] says.
    pub altstack: (u32, i32, u32),
}

impl SignalFrame {
    pub fn signo(&self) -> i32 {
        self.info.si_signo
    }

    /// `SA_RESTART`: an interrupted call restarts instead of failing with `EINTR`.
    pub fn restarts(&self) -> bool {
        self.flags & wasixv2_abi::SaFlags::SA_RESTART.0 != 0
    }

    /// `on_sig_stack`: whether the stack pointer `sp` lies on the alternate stack. A stack armed
    /// with `SS_AUTODISARM` never counts: delivery disarms it, so nothing runs on it armed.
    pub fn on_altstack(&self, sp: u32) -> bool {
        let (ss_sp, flags, size) = self.altstack;
        let autodisarm = flags as u32 & wasixv2_abi::SigaltstackFlags::SS_AUTODISARM.0 != 0;
        !autodisarm && sp > ss_sp && sp - ss_sp <= size
    }

    /// `sigsp`: where the frame goes for a guest interrupted with stack pointer `sp`. The top of
    /// the alternate stack when the handler asked for it (`SA_ONSTACK`), one is set and the
    /// guest is not on it already; `None` for below `sp`.
    pub fn altstack_top(&self, sp: u32) -> Option<u32> {
        let (ss_sp, _, size) = self.altstack;
        let onstack = self.flags & wasixv2_abi::SaFlags::SA_ONSTACK.0 != 0;
        (onstack && size != 0 && !self.on_altstack(sp)).then(|| ss_sp.wrapping_add(size))
    }
}

/// Set whenever the kernel leaves work for a task: an action, a signal, or a mask change that may
/// make a pending signal deliverable (Linux's `TIF_SIGPENDING` and `TIF_NOTIFY_RESUME`). The
/// execution layer reads it without a lock on every return to the guest and asks
/// [`Control::take_pending`] only while it is set; `take_pending` clears it before it looks, so
/// work queued meanwhile raises it again.
#[derive(Debug, Default)]
pub struct WorkFlag(AtomicBool);

impl WorkFlag {
    /// A flag that is always set, for implementations that do not track work per task.
    pub fn always() -> Self {
        Self(AtomicBool::new(true))
    }

    pub fn raise(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_raised(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The return-to-guest interface. The execution layer asks after every syscall and at every
/// preemption point, and reports what becomes of the instances it runs tasks on.
pub trait Control: AtomicWait {
    /// The next thing to do for `task`: an exit, a thread to start, a signal handler to run, or
    /// nothing.
    fn take_pending(&self, task: TaskId) -> Option<GuestAction>;

    /// The flag that says whether [`Control::take_pending`] may have something for `task`.
    fn work_flag(&self, task: TaskId) -> Arc<WorkFlag> {
        let _ = task;
        Arc::new(WorkFlag::always())
    }

    /// A handler from [`GuestAction::Signal`] finished. `mask` is `uc_sigmask` as the handler
    /// left it, which becomes the task's mask (`rt_sigreturn`), or `None` when the handler did
    /// not return but unwound past its frame (`siglongjmp`).
    fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        let _ = (task, mask);
    }

    /// The module bytes `task`'s pending [`GuestAction::Exec`] runs.
    fn take_image(&self, task: TaskId) -> Option<Vec<u8>> {
        let _ = task;
        None
    }

    /// Before the image of a [`GuestAction::Exec`] starts, once the instance left the old one:
    /// waits until the `execve` committed and `task` is the only task left in its thread group,
    /// as `de_thread` does. `Err(status)` when the group exited meanwhile, and the image must
    /// not start.
    fn exec_wait(&self, task: TaskId) -> LocalFuture<'_, Result<(), i32>> {
        let _ = task;
        Box::pin(async { Ok(()) })
    }

    /// Before the last task of a process is released: waits until the other tasks of its thread
    /// group are gone, as a zombie leader stays until its group is empty, so whoever waits for
    /// the process never sees it end while a thread still runs.
    fn group_wait(&self, task: TaskId) -> LocalFuture<'_, ()> {
        let _ = task;
        Box::pin(async {})
    }

    /// `task`'s instance trapped and cannot go on: the hardware fault `signal` (SIGSEGV, SIGILL,
    /// SIGFPE, SIGBUS) with no way to run a handler, so its default action kills the thread
    /// group (`force_sig_fault`, then `get_signal`). Leaves the group's exit pending on `task`.
    /// False when the group was exiting already: the trap came of tearing a thread down and
    /// changes nothing.
    fn fault(&self, task: TaskId, signal: i32) -> bool {
        let _ = (task, signal);
        true
    }

    /// The execution layer is done with `task`: its instance ended, or the vfork child stopped
    /// using its parent's (`release_task`). Nothing is asked about the task after this.
    fn release(&self, task: TaskId) {
        let _ = task;
    }

    /// The thread `task` from a [`GuestAction::StartThread`] could not be started: the kernel
    /// forgets it, as if `thread-spawn` had failed.
    fn abort_thread(&self, task: TaskId) {
        let _ = task;
    }
}

/// The word a wasm `memory.atomic.wait32` or `memory.atomic.wait64` expects at its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicWord {
    U32(u32),
    U64(u64),
}

/// Wasm's `memory.atomic.wait` and `memory.atomic.notify` instructions on the kernel's futexes.
/// The execution layer turns the instructions into calls of these, so a guest blocked in one takes
/// signals and its group's exit as one blocked in `futex` does, and the instructions and the
/// syscall wake each other's waiters on the same address.
pub trait AtomicWait {
    /// `memory.atomic.wait32` or `wait64` of the calling task at `addr`, which is aligned to the
    /// size of `expected` and in bounds: a private `FUTEX_WAIT` that compares a word of that
    /// size, for `timeout` if given. 0 when woken, `EAGAIN` when the word does not hold
    /// `expected`, `ETIMEDOUT`; a signal interrupts it as it does `FUTEX_WAIT`.
    fn atomic_wait<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> LocalFuture<'a, SysResult<i32>>;

    /// `memory.atomic.notify` of `task` at `addr`: wakes up to `count` waiters there, of either
    /// instruction or of `futex`, and returns how many it woke. Unlike `FUTEX_WAKE`, a count of
    /// 0 wakes none.
    fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32;
}

/// The executable formats `execve` can start besides `#!` scripts, which the kernel handles
/// itself: Linux's binary format handlers, as `search_binary_handler` asks them. The kernel asks
/// before the point of no return, so an image nothing runs fails `execve` (with `ENOEXEC`) and
/// leaves the caller's image in place, as an unknown binary does on Linux.
pub trait Binfmt: Send + Sync {
    /// Whether `image`, the contents of an executable file, is a program a handler starts, from
    /// a cheap look at it (its header, its import section). The errno `execve` fails with when
    /// it is not, normally `ENOEXEC`.
    fn probe(&self, image: &[u8]) -> Result<(), Errno>;
}

// By hand rather than generated with the rest of `Layered`: the layers see syscalls, and these
// are instructions.
impl<L, S: AtomicWait> AtomicWait for Layered<L, S> {
    fn atomic_wait<'a>(
        &'a self,
        caller: &'a Caller<'a>,
        addr: u32,
        expected: AtomicWord,
        timeout: Option<Duration>,
    ) -> LocalFuture<'a, SysResult<i32>> {
        self.inner.atomic_wait(caller, addr, expected, timeout)
    }

    fn atomic_notify(&self, task: TaskId, addr: u32, count: u32) -> u32 {
        self.inner.atomic_notify(task, addr, count)
    }
}
