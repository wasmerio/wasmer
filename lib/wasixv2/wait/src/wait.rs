use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

use crate::{PollMask, PollTable, Pollable, WaitEntry, WaitQueue, WaitRegistration};

/// How the kernel tells a waiting task that a signal arrived: Linux's `signal_pending` plus the
/// queue that signal delivery wakes. Blocking waits give up with `Interrupted` once `pending`
/// is true.
pub trait Interrupt {
    fn pending(&self) -> bool;

    /// Woken when `pending` may have become true.
    fn queue(&self) -> Option<&WaitQueue> {
        None
    }
}

/// For waits nothing can interrupt.
pub struct NoInterrupt;

impl Interrupt for NoInterrupt {
    fn pending(&self) -> bool {
        false
    }
}

/// A wait ended by a pending signal; the syscall returns `EINTR` or restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interrupted;

/// Why `Waiter::sleep` returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wakeup {
    /// A queue the waiter is on was woken with these events (accumulated since the last wakeup).
    Woken(PollMask),
    Interrupted,
    TimedOut,
}

/// A sleeping task: the wait entry that wakes a Rust `Waker`, as Linux's
/// `prepare_to_wait`/`schedule` pair does for a task. One waiter can sit on many queues.
pub struct Waiter {
    state: Arc<TaskState>,
}

#[derive(Default)]
struct TaskState {
    inner: Mutex<TaskInner>,
}

#[derive(Default)]
struct TaskInner {
    woken: bool,
    events: PollMask,
    waker: Option<Waker>,
}

impl TaskState {
    fn wake(&self, events: PollMask) {
        let waker = {
            let mut inner = self.inner.lock().unwrap();
            inner.woken = true;
            inner.events |= events;
            inner.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// The waiter's entry on one queue, with that registration's key.
struct KeyedEntry {
    key: PollMask,
    exclusive: bool,
    task: Arc<TaskState>,
}

impl WaitEntry for KeyedEntry {
    /// Linux's `pollwake` and `autoremove_wake_function`: ignore wakeups for events nobody asked
    /// about.
    fn wake(&self, events: PollMask) -> bool {
        if !self.key.is_empty() && !events.is_empty() && !events.intersects(self.key) {
            return false;
        }
        self.task.wake(events);
        true
    }

    fn exclusive(&self) -> bool {
        self.exclusive
    }
}

impl Default for Waiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Waiter {
    pub fn new() -> Self {
        Self {
            state: Arc::default(),
        }
    }

    /// Puts the waiter on `queue`, woken by events in `key` (empty: any event).
    pub fn register(&self, queue: &WaitQueue, key: PollMask) -> WaitRegistration {
        self.add(queue, key, false)
    }

    /// As `register`, as an exclusive entry (`prepare_to_wait_exclusive`).
    pub fn register_exclusive(&self, queue: &WaitQueue, key: PollMask) -> WaitRegistration {
        self.add(queue, key, true)
    }

    fn add(&self, queue: &WaitQueue, key: PollMask, exclusive: bool) -> WaitRegistration {
        queue.add(Arc::new(KeyedEntry {
            key,
            exclusive,
            task: self.state.clone(),
        }))
    }

    /// Clears and returns the events of wakeups not yet consumed, if there were any.
    pub fn take_wakeup(&self) -> Option<PollMask> {
        let mut inner = self.state.inner.lock().unwrap();
        if !inner.woken {
            return None;
        }
        inner.woken = false;
        Some(std::mem::take(&mut inner.events))
    }

    /// `schedule()`: completes once the waiter is woken (consuming the wakeup), `interrupt`
    /// becomes pending, or `timer` completes, checked in that order. Returns at once if a
    /// wakeup already arrived since the last one was consumed.
    ///
    /// Cancellation-safe: dropping the future loses nothing; an unconsumed wakeup stays pending.
    pub async fn sleep(
        &self,
        interrupt: &dyn Interrupt,
        mut timer: Option<Pin<&mut (dyn Future<Output = ()> + '_)>>,
    ) -> Wakeup {
        let _interrupt = interrupt
            .queue()
            .map(|queue| self.register(queue, PollMask::empty()));
        poll_fn(|cx| {
            {
                let mut inner = self.state.inner.lock().unwrap();
                if inner.woken {
                    inner.woken = false;
                    return Poll::Ready(Wakeup::Woken(std::mem::take(&mut inner.events)));
                }
                inner.waker = Some(cx.waker().clone());
            }
            if interrupt.pending() {
                return Poll::Ready(Wakeup::Interrupted);
            }
            if let Some(timer) = timer.as_mut()
                && timer.as_mut().poll(cx).is_ready()
            {
                return Poll::Ready(Wakeup::TimedOut);
            }
            Poll::Pending
        })
        .await
    }
}

/// Linux's `poll_wqueues`: one waiter plus its registrations on every queue the polled objects
/// named. Used by poll and select; dropping it is `poll_freewait`.
#[derive(Default)]
pub struct PollWaiter {
    waiter: Waiter,
    registrations: Vec<WaitRegistration>,
}

impl PollWaiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `vfs_poll`: queries `object`, and with `register` also puts the waiter on its queues for
    /// the events in `key`.
    pub fn poll(&mut self, object: &dyn Pollable, key: PollMask, register: bool) -> PollMask {
        if !register {
            return object.poll(None);
        }
        let waiter = &self.waiter;
        let registrations = &mut self.registrations;
        let mut queue_proc = |queue: &WaitQueue, key: PollMask| {
            registrations.push(waiter.register(queue, key));
        };
        object.poll(Some(&mut PollTable::new(key, &mut queue_proc)))
    }

    pub fn waiter(&self) -> &Waiter {
        &self.waiter
    }

    /// See `Waiter::sleep`.
    pub async fn sleep(
        &self,
        interrupt: &dyn Interrupt,
        timer: Option<Pin<&mut (dyn Future<Output = ()> + '_)>>,
    ) -> Wakeup {
        self.waiter.sleep(interrupt, timer).await
    }
}

/// `wait_event_interruptible`: waits on `queue` until `ready` returns a value, re-checking it
/// after every wakeup. The task is on the queue before the first check, so no wakeup is lost.
///
/// Cancellation-safe: dropping the future removes the task from the queue.
pub async fn wait_event<T>(
    queue: &WaitQueue,
    ready: impl FnMut() -> Option<T>,
    interrupt: &dyn Interrupt,
) -> Result<T, Interrupted> {
    let result = wait(queue, false, ready, interrupt, None).await;
    result.map(|value| value.expect("no timer to expire"))
}

/// `wait_event_interruptible_timeout`: as `wait_event`, but gives up with `Ok(None)` once
/// `timer` completes (`ready` is checked one last time first).
pub async fn wait_event_timeout<T>(
    queue: &WaitQueue,
    ready: impl FnMut() -> Option<T>,
    interrupt: &dyn Interrupt,
    timer: impl Future<Output = ()>,
) -> Result<Option<T>, Interrupted> {
    let timer = std::pin::pin!(timer);
    wait(queue, false, ready, interrupt, Some(timer)).await
}

/// `wait_event_interruptible_exclusive`: as `wait_event`, as an exclusive waiter, so a wake
/// reaches one such task. A wakeup the task took but did not use, because it was interrupted
/// or dropped, is passed on to the next exclusive waiter, as Linux's `abort_exclusive_wait`.
pub async fn wait_event_exclusive<T>(
    queue: &WaitQueue,
    ready: impl FnMut() -> Option<T>,
    interrupt: &dyn Interrupt,
) -> Result<T, Interrupted> {
    let result = wait(queue, true, ready, interrupt, None).await;
    result.map(|value| value.expect("no timer to expire"))
}

async fn wait<T>(
    queue: &WaitQueue,
    exclusive: bool,
    mut ready: impl FnMut() -> Option<T>,
    interrupt: &dyn Interrupt,
    mut timer: Option<Pin<&mut (dyn Future<Output = ()> + '_)>>,
) -> Result<Option<T>, Interrupted> {
    let mut guard = WaitGuard {
        queue,
        waiter: Waiter::new(),
        registration: None,
        exclusive,
    };
    guard.registration = Some(guard.waiter.add(queue, PollMask::empty(), exclusive));
    loop {
        // A wakeup is consumed here, by checking the condition; any wakeup still pending when
        // the guard drops arrived during the last sleep and was never acted on.
        guard.waiter.take_wakeup();
        if let Some(value) = ready() {
            return Ok(Some(value));
        }
        if interrupt.pending() {
            return Err(Interrupted);
        }
        let timer = timer.as_mut().map(|timer| timer.as_mut());
        match guard.waiter.sleep(interrupt, timer).await {
            Wakeup::Woken(_) => {}
            Wakeup::Interrupted => return Err(Interrupted),
            Wakeup::TimedOut => return Ok(ready()),
        }
    }
}

struct WaitGuard<'a> {
    queue: &'a WaitQueue,
    waiter: Waiter,
    registration: Option<WaitRegistration>,
    exclusive: bool,
}

impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        self.registration.take();
        if self.exclusive
            && let Some(events) = self.waiter.take_wakeup()
        {
            self.queue.wake(events);
        }
    }
}
