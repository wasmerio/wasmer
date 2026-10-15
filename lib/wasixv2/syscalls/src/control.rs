use wasixv2_abi::Siginfo;

use crate::caller::TaskId;

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
    /// Where the frame goes: the top of the alternate stack, or `None` for below the interrupted
    /// stack pointer.
    pub stack_top: Option<u32>,
    /// The alternate stack settings as `sigaltstack` reports them: `ss_sp`, `ss_flags`,
    /// `ss_size`.
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
}

/// The return-to-guest interface. The execution layer asks after every syscall.
pub trait Control {
    /// The next thing to do for `task`: an exit, a thread to start, a signal handler to run, or
    /// nothing.
    fn take_pending(&self, task: TaskId) -> Option<GuestAction>;

    /// A handler from [`GuestAction::Signal`] finished. `mask` is `uc_sigmask` as the handler
    /// left it, which becomes the task's mask (`rt_sigreturn`), or `None` when the handler did
    /// not return but unwound past its frame (`siglongjmp`).
    fn signal_return(&self, task: TaskId, mask: Option<u64>) {
        let _ = (task, mask);
    }
}
