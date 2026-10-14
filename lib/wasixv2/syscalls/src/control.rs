use crate::caller::TaskId;

/// Work the execution layer must do before control returns to the guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestAction {
    /// The task's thread group exited with this status.
    Exit(i32),
}

/// The return-to-guest interface. The execution layer asks after every syscall.
pub trait Control {
    fn take_pending(&self, task: TaskId) -> Option<GuestAction>;
}
