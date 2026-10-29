//! Task states for blocking waits (`set_current_state`, `schedule`, `try_to_wake_up`).

use std::{
    future::{Future, poll_fn},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    task::{Context, Wake, Waker},
};

/// The scheduling state of a live task. Exit and job-control states belong to the kernel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u64)]
pub enum TaskState {
    #[default]
    Running,
    Interruptible,
    Uninterruptible,
    Idle,
}

/// The state shared by a task's waits and observers such as procfs.
#[derive(Clone, Default)]
pub struct TaskStatus(Arc<AtomicU64>);

const STATE_MASK: u64 = 3;
const PENDING: u64 = 4;
const STATUS_MASK: u64 = STATE_MASK | PENDING;

/// A wait's immutable forwarding chain. A task-wide cached parent could point back into
/// itself when a nested wait receives an intermediate waker that forwards to the outer one.
struct TaskWake {
    status: TaskStatus,
    parent: Waker,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // kernel/sched/core.c try_to_wake_up: runnable at wakeup, before the next poll.
        let _ = self.status.0.fetch_update(Relaxed, Relaxed, |old| {
            Some((old & !STATUS_MASK).wrapping_add(STATUS_MASK + 1))
        });
        self.parent.wake_by_ref();
    }
}

impl TaskStatus {
    pub fn state(&self) -> TaskState {
        match self.0.load(Relaxed) & STATE_MASK {
            1 => TaskState::Interruptible,
            2 => TaskState::Uninterruptible,
            3 => TaskState::Idle,
            _ => TaskState::Running,
        }
    }

    /// Reports `state` while `future` is pending; nested waits can supply a more specific
    /// state. During polling the task is running. Wakeup, completion and cancellation make
    /// it runnable. One task's futures must not be polled concurrently, like the task itself.
    pub async fn during<T>(&self, state: TaskState, future: impl Future<Output = T>) -> T {
        struct Running<'a>(&'a TaskStatus);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                self.0.0.fetch_and(!STATUS_MASK, Relaxed);
            }
        }
        let _running = Running(self);
        let mut future = pin!(future);
        let mut wake: Option<Arc<TaskWake>> = None;
        poll_fn(|cx| {
            if wake
                .as_ref()
                .is_none_or(|wake| !wake.parent.will_wake(cx.waker()))
            {
                wake = Some(Arc::new(TaskWake {
                    status: self.clone(),
                    parent: cx.waker().clone(),
                }));
            }
            let waker = Waker::from(wake.as_ref().unwrap().clone());
            // Low bits: state and a pending marker. High bits: wake generation, so a wake
            // during poll cannot be lost when Pending publishes the sleeping state.
            let before = self.0.fetch_and(!STATUS_MASK, Relaxed) & !STATUS_MASK;
            let result = future.as_mut().poll(&mut Context::from_waker(&waker));
            if result.is_pending() {
                // Preserve a nested wait's state (including runnable, for CPU throttling),
                // or a concurrent wake that advanced the generation.
                let _ = self.0.compare_exchange(
                    before,
                    before | PENDING | state as u64,
                    Relaxed,
                    Relaxed,
                );
            }
            result
        })
        .await
    }
}
