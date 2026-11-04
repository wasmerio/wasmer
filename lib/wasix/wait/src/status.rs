//! Task states for blocking waits (`set_current_state`, `schedule`, `try_to_wake_up`).

use std::{
    future::{Future, poll_fn},
    mem::ManuallyDrop,
    pin::pin,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    task::{Context, RawWaker, RawWakerVTable, Wake, Waker},
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

fn notify(status: &TaskStatus, parent: &Waker) {
    // kernel/sched/core.c:try_to_wake_up makes a task runnable at wakeup.
    let _ = status.0.fetch_update(Relaxed, Relaxed, |old| {
        Some((old & !STATUS_MASK).wrapping_add(STATUS_MASK + 1))
    });
    parent.wake_by_ref();
}

/// Borrowed only during one poll. A clone always owns an ordinary Arc<TaskWake>.
struct BorrowedWake<'a> {
    status: &'a TaskStatus,
    parent: &'a Waker,
    owned: &'a OnceLock<Arc<TaskWake>>,
}

impl BorrowedWake<'_> {
    const VTABLE: RawWakerVTable =
        RawWakerVTable::new(Self::clone_owned, Self::wake, Self::wake, |_| {});

    unsafe fn clone_owned(data: *const ()) -> RawWaker {
        // SAFETY: the borrowed waker is used only while its BorrowedWake is live.
        let this = unsafe { &*data.cast::<BorrowedWake<'_>>() };
        let owned = this.owned.get_or_init(|| {
            Arc::new(TaskWake {
                status: this.status.clone(),
                parent: this.parent.clone(),
            })
        });
        // Transfer this owning Waker's raw parts; its standard Arc vtable drops
        // the clone, independently of the borrowed poll's lifetime.
        let waker = ManuallyDrop::new(Waker::from(owned.clone()));
        RawWaker::new(waker.data(), waker.vtable())
    }

    unsafe fn wake(data: *const ()) {
        // SAFETY: clones use the owning Arc vtable, so only the live borrowed
        // poll can reach this callback. No stack allocation is freed here.
        let this = unsafe { &*data.cast::<BorrowedWake<'_>>() };
        notify(this.status, this.parent);
    }
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        notify(&self.status, &self.parent);
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
            // Low bits: state and a pending marker. High bits: wake generation, so a wake
            // during poll cannot be lost when Pending publishes the sleeping state.
            let before = self.0.fetch_and(!STATUS_MASK, Relaxed) & !STATUS_MASK;
            let result = match wake
                .as_ref()
                .filter(|wake| wake.parent.will_wake(cx.waker()))
            {
                Some(wake) => {
                    let waker = Waker::from(wake.clone());
                    future.as_mut().poll(&mut Context::from_waker(&waker))
                }
                None => {
                    let owned = OnceLock::new();
                    let borrowed = BorrowedWake {
                        status: self,
                        parent: cx.waker(),
                        owned: &owned,
                    };
                    let raw = RawWaker::new(
                        (&borrowed as *const BorrowedWake<'_>).cast(),
                        &BorrowedWake::VTABLE,
                    );
                    // SAFETY: this waker is lent only to the poll below and is
                    // never moved out; any clone has the owning Arc vtable.
                    let waker = ManuallyDrop::new(unsafe { Waker::from_raw(raw) });
                    let result = future.as_mut().poll(&mut Context::from_waker(&waker));
                    wake = owned.into_inner();
                    result
                }
            };
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
