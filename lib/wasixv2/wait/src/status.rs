//! Task states for blocking waits (`set_current_state`, `schedule`, `try_to_wake_up`).

use std::{
    future::{Future, poll_fn},
    mem::ManuallyDrop,
    pin::pin,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
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
pub struct TaskStatus(Arc<Mutex<Projection>>);

#[derive(Default)]
struct Projection {
    current: Weak<WaitRecord>,
    polling: Weak<WaitRecord>,
    state: TaskState,
}

struct WaitRecord {
    parent: Weak<WaitRecord>,
    generation: AtomicU64,
    retired: AtomicBool,
}

impl TaskStatus {
    fn projection(&self) -> MutexGuard<'_, Projection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A wait's immutable forwarding chain. A task-wide cached parent could point back into
/// itself when a nested wait receives an intermediate waker that forwards to the outer one.
struct TaskWake {
    status: TaskStatus,
    parent: Waker,
    record: Arc<WaitRecord>,
}

fn notify(status: &TaskStatus, record: &Arc<WaitRecord>, parent: &Waker) {
    {
        let mut projection = status.projection();
        record.generation.fetch_add(1, Relaxed);
        let current = projection.current.upgrade();
        let mut ancestor = current.clone();
        while let Some(wait) = ancestor {
            if wait.retired.load(Relaxed) {
                break;
            }
            if Arc::ptr_eq(&wait, record) {
                if let Some(current) = &current
                    && !Arc::ptr_eq(current, record)
                {
                    current.generation.fetch_add(1, Relaxed);
                }
                projection.state = TaskState::Running;
                break;
            }
            ancestor = wait.parent.upgrade();
        }
    }
    parent.wake_by_ref();
}

/// Borrowed only during one poll. A clone always owns an ordinary Arc<TaskWake>.
struct BorrowedWake<'a> {
    status: &'a TaskStatus,
    record: &'a Arc<WaitRecord>,
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
                record: this.record.clone(),
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
        notify(this.status, this.record, this.parent);
    }
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        notify(&self.status, &self.record, &self.parent);
    }
}

impl TaskStatus {
    pub fn state(&self) -> TaskState {
        self.projection().state
    }

    /// Reports `state` while `future` is pending; nested waits can supply a more specific
    /// state. During polling the task is running. Wakeup, completion and cancellation make
    /// it runnable. One task's futures must not be polled concurrently, like the task itself.
    pub async fn during<T>(&self, state: TaskState, future: impl Future<Output = T>) -> T {
        struct Running<'a> {
            status: &'a TaskStatus,
            record: Arc<WaitRecord>,
        }
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                let mut projection = self.status.projection();
                self.record.retired.store(true, Relaxed);
                if projection.current.ptr_eq(&Arc::downgrade(&self.record)) {
                    // A dead immediate parent is never resurrected through an older ancestor.
                    projection.current = self
                        .record
                        .parent
                        .upgrade()
                        .filter(|parent| !parent.retired.load(Relaxed))
                        .map_or_else(Weak::new, |parent| Arc::downgrade(&parent));
                    projection.state = TaskState::Running;
                }
            }
        }
        let running = Running {
            status: self,
            record: Arc::new(WaitRecord {
                parent: self.projection().polling.clone(),
                generation: AtomicU64::new(0),
                retired: AtomicBool::new(false),
            }),
        };
        let mut future = pin!(future);
        let mut wake: Option<Arc<TaskWake>> = None;
        poll_fn(|cx| {
            struct Polling<'a> {
                status: &'a TaskStatus,
                previous: Weak<WaitRecord>,
            }
            impl Drop for Polling<'_> {
                fn drop(&mut self) {
                    self.status.projection().polling = self.previous.clone();
                }
            }
            let (before, _polling) = {
                let mut projection = self.projection();
                projection.current = Arc::downgrade(&running.record);
                projection.state = TaskState::Running;
                let previous =
                    std::mem::replace(&mut projection.polling, Arc::downgrade(&running.record));
                (
                    running.record.generation.load(Relaxed),
                    Polling {
                        status: self,
                        previous,
                    },
                )
            };
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
                        record: &running.record,
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
                let mut projection = self.projection();
                // Nested waits own their projection even when they report Running.
                // A sibling's wake or destruction cannot overwrite this record.
                if projection.current.ptr_eq(&Arc::downgrade(&running.record))
                    && running.record.generation.load(Relaxed) == before
                {
                    projection.state = state;
                }
            }
            result
        })
        .await
    }
}
