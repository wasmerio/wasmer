//! Readiness for the WASIX kernel: poll masks, wait queues, poll tables and blocking waits.
//!
//! The Rust counterpart of Linux's `wait_queue_head_t`, `poll_table` and `__poll_t`
//! (`include/linux/wait.h`, `include/linux/poll.h`). Objects (pipes, sockets, host streams) own
//! `WaitQueue`s and wake them when their state changes, from any thread. Pollers (blocking reads,
//! poll, select, epoll) put entries on those queues through a `PollTable` or a `Waiter`. Waiting
//! integrates with Rust wakers, so it works on any executor.
//!
//! A separate crate because the VFS, the network and the kernel all need it, and the guest ABI
//! is their only common dependency.

mod mask;
mod poll;
mod queue;
mod status;
mod wait;

pub use mask::PollMask;
pub use poll::{PollTable, Pollable, poll_wait};
pub use queue::{WaitEntry, WaitQueue, WaitRegistration};
pub use status::{TaskState, TaskStatus};
pub use wait::{
    Interrupt, Interrupted, NoInterrupt, PollWaiter, Waiter, WakeHandle, Wakeup, Watched,
    interruptible, wait_event, wait_event_exclusive, wait_event_timeout,
};

#[cfg(test)]
mod tests;
