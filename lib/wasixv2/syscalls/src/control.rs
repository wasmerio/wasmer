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
}

/// The return-to-guest interface. The execution layer asks after every syscall.
pub trait Control {
    fn take_pending(&self, task: TaskId) -> Option<GuestAction>;
}
