//! OS task management for processes and threads.

pub mod control_plane;
pub mod process;
pub mod signal;
mod task_join_handle;
pub mod thread;

pub use task_join_handle::{
    OwnedTaskStatus, TaskJoinHandle, TaskStatus, TaskTerminatedError, VirtualTaskHandle,
};
