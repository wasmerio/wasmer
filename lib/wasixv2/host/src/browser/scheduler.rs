use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use crate::LocalFuture;

/// A runtime scheduler word, unrelated to guest futex addresses or kernel futex keys.
/// Sequence publication follows Linux kernel/sched/wait.c:prepare_to_wait_event and
/// kernel/sched/core.c:try_to_wake_up: establish the predicate before sleeping and recheck it.
#[derive(Default)]
pub struct Parker {
    sequence: AtomicI32,
    #[cfg(not(target_arch = "wasm32"))]
    lock: std::sync::Mutex<()>,
    #[cfg(not(target_arch = "wasm32"))]
    changed: std::sync::Condvar,
}

impl Parker {
    /// Snapshot before polling/registering the future's waker.
    pub fn sequence(&self) -> i32 {
        self.sequence.load(Ordering::Acquire)
    }

    /// Publish readiness from any worker without touching a worker-local JS task queue.
    pub fn notify(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        let _guard = self.lock.lock().unwrap();
        self.sequence.fetch_add(1, Ordering::Release);
        #[cfg(target_arch = "wasm32")]
        super::notify_word(&self.sequence);
        #[cfg(not(target_arch = "wasm32"))]
        self.changed.notify_all();
    }

    /// Park only if no wake occurred since `observed`. Spurious wakes are permitted.
    /// Browser callers must be dedicated task/service workers, never the page main thread.
    pub fn park(&self, observed: i32) {
        #[cfg(target_arch = "wasm32")]
        super::wait_word(&self.sequence, observed);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut guard = self.lock.lock().unwrap();
            while self.sequence() == observed {
                guard = self.changed.wait(guard).unwrap();
            }
        }
    }
}

impl Wake for Parker {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}

struct Ready {
    ready: AtomicBool,
    parker: Arc<Parker>,
}

impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        self.parker.notify();
    }
}

struct Task {
    future: LocalFuture<'static, ()>,
    ready: Arc<Ready>,
}

#[derive(Default)]
struct Scheduler {
    parker: Arc<Parker>,
    tasks: RefCell<VecDeque<Task>>,
}

thread_local! {
    static LOCAL: RefCell<Option<std::rc::Rc<Scheduler>>> = const { RefCell::new(None) };
}

fn current() -> std::rc::Rc<Scheduler> {
    LOCAL.with_borrow(|local| {
        local
            .clone()
            .expect("BrowserExecutor requires a worker driver")
    })
}

pub(super) fn spawn(future: LocalFuture<'static, ()>) -> std::io::Result<()> {
    let scheduler = current();
    if scheduler.tasks.borrow().len() >= 1024 {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    let ready = Arc::new(Ready {
        ready: AtomicBool::new(true),
        parker: scheduler.parker.clone(),
    });
    scheduler
        .tasks
        .borrow_mut()
        .push_back(Task { future, ready });
    scheduler.parker.notify();
    Ok(())
}

/// Poll a future synchronously on its owning worker, parking on runtime scheduler words
/// on Pending. B10 may nest this inside a synchronous host import. No JS Promise may
/// require this worker's event loop while this function runs.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    let owner = LOCAL.with_borrow_mut(|local| {
        if local.is_some() {
            false
        } else {
            *local = Some(std::rc::Rc::new(Scheduler::default()));
            true
        }
    });
    struct Scope(bool);
    impl Drop for Scope {
        fn drop(&mut self) {
            if self.0 {
                let scheduler = LOCAL.with_borrow_mut(Option::take).unwrap();
                assert!(
                    scheduler.tasks.borrow().is_empty(),
                    "worker exited with local tasks"
                );
            }
        }
    }
    let _scope = Scope(owner);
    let scheduler = current();
    let waker = Waker::from(scheduler.parker.clone());
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    let mut result = None;
    loop {
        let observed = scheduler.parker.sequence();
        let count = scheduler.tasks.borrow().len();
        for _ in 0..count {
            let Some(mut task) = scheduler.tasks.borrow_mut().pop_front() else {
                break;
            };
            // No RefCell borrow or runtime lock survives a poll or nested host call.
            let complete = task.ready.ready.swap(false, Ordering::Acquire)
                && task
                    .future
                    .as_mut()
                    .poll(&mut Context::from_waker(&Waker::from(task.ready.clone())))
                    .is_ready();
            if !complete {
                scheduler.tasks.borrow_mut().push_back(task);
            }
        }
        if result.is_none()
            && let Poll::Ready(value) = future.as_mut().poll(&mut cx)
        {
            result = Some(value);
        }
        if (!owner || scheduler.tasks.borrow().is_empty())
            && let Some(value) = result.take()
        {
            return value;
        }
        scheduler.parker.park(observed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn detached_local_queue_has_a_resource_bound_and_drains_before_exit() {
        block_on(async {
            for _ in 0..1024 {
                spawn(Box::pin(async {})).unwrap();
            }
            assert_eq!(
                spawn(Box::pin(async {})).unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
    }

    #[test]
    fn wake_before_wait_is_retained() {
        let parker = Parker::default();
        let observed = parker.sequence();
        parker.notify();
        parker.park(observed);
    }

    #[test]
    fn foreign_waker_polls_non_send_future_on_its_owner() {
        let slot = Arc::new(Mutex::new(None::<Waker>));
        let ready = Arc::new(AtomicBool::new(false));
        let send_slot = slot.clone();
        let send_ready = ready.clone();
        let thread = std::thread::spawn(move || {
            loop {
                if let Some(waker) = send_slot.lock().unwrap().take() {
                    send_ready.store(true, Ordering::Release);
                    waker.wake();
                    break;
                }
                std::thread::yield_now();
            }
        });
        let owner = std::thread::current().id();
        let local = std::rc::Rc::new(42);
        let result = block_on(std::future::poll_fn(|cx| {
            assert_eq!(std::thread::current().id(), owner);
            *slot.lock().unwrap() = Some(cx.waker().clone());
            if ready.load(Ordering::Acquire) {
                Poll::Ready(*local)
            } else {
                Poll::Pending
            }
        }));
        thread.join().unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn nested_background_poll_can_drain_the_outer_ready_set() {
        block_on(async {
            spawn(Box::pin(async {
                block_on(async {});
            }))
            .unwrap();
            spawn(Box::pin(async {})).unwrap();
        });
    }

    #[test]
    fn local_tasks_and_nested_block_on_share_the_worker_scheduler() {
        let ran = std::rc::Rc::new(AtomicBool::new(false));
        block_on(async {
            let flag = ran.clone();
            spawn(Box::pin(async move { flag.store(true, Ordering::Release) })).unwrap();
            block_on(std::future::poll_fn(|_| {
                if ran.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }));
        });
        // A reused worker starts with an empty local scheduler.
        assert_eq!(block_on(async { 7 }), 7);
    }
}
