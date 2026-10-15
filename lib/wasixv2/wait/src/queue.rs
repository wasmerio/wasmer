use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::PollMask;

/// Something to run when a `WaitQueue` is woken: a sleeping task, a poll or epoll registration.
/// Linux's `wait_queue_entry_t` with its `func`.
pub trait WaitEntry: Send + Sync {
    /// Called from `WaitQueue::wake`, possibly on another thread (a host reactor, say). Must not
    /// block or wake the same queue. `events` is the wake key; empty means "anything may have
    /// changed", like Linux's `wake_up` with a NULL key, and every entry must treat it as a
    /// match.
    ///
    /// Returns whether the entry took the wakeup. Only exclusive entries' answers matter: the
    /// walk stops at the first exclusive entry that took it.
    fn wake(&self, events: PollMask) -> bool;

    /// Exclusive entries (`WQ_FLAG_EXCLUSIVE`) queue after every non-exclusive one, and a wake
    /// reaches only the first exclusive entry that takes it, as `accept` waiters need.
    fn exclusive(&self) -> bool {
        false
    }
}

/// Linux's `wait_queue_head_t`. Objects own one (or more) and call `wake` when their state
/// changes. Cloning gives another handle to the same queue.
#[derive(Clone, Default)]
pub struct WaitQueue {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Non-exclusive entries first, then exclusive ones in the order they were added.
    entries: Mutex<VecDeque<(u64, Arc<dyn WaitEntry>)>>,
    last_id: AtomicU64,
}

impl WaitQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `add_wait_queue` and `add_wait_queue_exclusive`, as the entry says. The returned guard
    /// removes the entry when dropped.
    pub fn add(&self, entry: Arc<dyn WaitEntry>) -> WaitRegistration {
        let id = self.inner.last_id.fetch_add(1, Ordering::Relaxed) + 1;
        let mut entries = self.inner.entries.lock().unwrap();
        if entry.exclusive() {
            entries.push_back((id, entry));
        } else {
            entries.push_front((id, entry));
        }
        WaitRegistration {
            queue: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// `__wake_up` with `events` as the key and one exclusive wakeup: runs every non-exclusive
    /// entry, then exclusive entries until one takes the wakeup.
    ///
    /// Entries run outside the queue's lock, so they may add or remove registrations. An entry
    /// whose registration is dropped while a wake is in flight may still run once.
    pub fn wake(&self, events: PollMask) {
        let entries: Vec<_> = {
            let entries = self.inner.entries.lock().unwrap();
            if entries.is_empty() {
                return;
            }
            entries.iter().map(|(_, entry)| entry.clone()).collect()
        };
        for entry in entries {
            if entry.wake(events) && entry.exclusive() {
                break;
            }
        }
    }

    /// `waitqueue_active`: whether any entry is registered.
    pub fn is_active(&self) -> bool {
        !self.inner.entries.lock().unwrap().is_empty()
    }

    /// Whether `self` and `other` are handles to the same queue.
    pub fn same_queue(&self, other: &WaitQueue) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for WaitQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.inner.entries.lock().unwrap().len();
        f.debug_struct("WaitQueue").field("entries", &len).finish()
    }
}

/// An entry's place on a `WaitQueue`; dropping it is `remove_wait_queue`. It does not keep the
/// queue alive.
#[must_use = "dropping the registration removes the entry at once"]
#[derive(Debug)]
pub struct WaitRegistration {
    queue: Weak<Inner>,
    id: u64,
}

impl WaitRegistration {
    /// Whether this registration is on `queue`.
    pub fn is_on(&self, queue: &WaitQueue) -> bool {
        self.queue.as_ptr() == Arc::as_ptr(&queue.inner)
    }
}

impl Drop for WaitRegistration {
    fn drop(&mut self) {
        let Some(queue) = self.queue.upgrade() else {
            return;
        };
        // Taken out under the lock but dropped after it, in case the entry owns registrations.
        let removed = {
            let mut entries = queue.entries.lock().unwrap();
            let index = entries.iter().position(|(id, _)| *id == self.id);
            index.and_then(|index| entries.remove(index))
        };
        drop(removed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[derive(Default)]
    struct Counter {
        exclusive: bool,
        accept: bool,
        hits: AtomicUsize,
        last: Mutex<PollMask>,
    }

    impl WaitEntry for Counter {
        fn wake(&self, events: PollMask) -> bool {
            self.hits.fetch_add(1, Ordering::SeqCst);
            *self.last.lock().unwrap() = events;
            self.accept
        }

        fn exclusive(&self) -> bool {
            self.exclusive
        }
    }

    fn counter(exclusive: bool, accept: bool) -> Arc<Counter> {
        Arc::new(Counter {
            exclusive,
            accept,
            ..Counter::default()
        })
    }

    fn hits(entry: &Counter) -> usize {
        entry.hits.load(Ordering::SeqCst)
    }

    #[test]
    fn wake_reaches_every_entry_with_the_key() {
        let queue = WaitQueue::new();
        let a = counter(false, true);
        let b = counter(false, false);
        let _ra = queue.add(a.clone());
        let _rb = queue.add(b.clone());

        queue.wake(PollMask::READABLE);

        assert_eq!((hits(&a), hits(&b)), (1, 1));
        assert_eq!(*b.last.lock().unwrap(), PollMask::READABLE);
    }

    #[test]
    fn exclusive_entries_stop_at_the_first_taker() {
        let queue = WaitQueue::new();
        let declines = counter(true, false);
        let first = counter(true, true);
        let second = counter(true, true);
        let shared = counter(false, true);
        let _r1 = queue.add(declines.clone());
        let _r2 = queue.add(first.clone());
        let _r3 = queue.add(second.clone());
        // Added last but runs first: non-exclusive entries precede exclusive ones.
        let _r4 = queue.add(shared.clone());

        queue.wake(PollMask::IN);

        assert_eq!(
            [&declines, &first, &second, &shared].map(|e| hits(e)),
            [1, 1, 0, 1]
        );
    }

    #[test]
    fn dropping_the_registration_removes_the_entry() {
        let queue = WaitQueue::new();
        let entry = counter(false, true);
        let registration = queue.add(entry.clone());
        assert!(queue.is_active());
        assert!(registration.is_on(&queue));

        drop(registration);
        queue.wake(PollMask::IN);

        assert!(!queue.is_active());
        assert_eq!(hits(&entry), 0);
    }

    #[test]
    fn registration_outlives_its_queue() {
        let queue = WaitQueue::new();
        let registration = queue.add(counter(false, true));
        drop(queue);
        drop(registration);
    }

    /// An entry that removes its own registration from inside `wake`, as a one-shot waiter does.
    struct SelfRemoving(Mutex<Option<WaitRegistration>>);

    impl WaitEntry for SelfRemoving {
        fn wake(&self, _: PollMask) -> bool {
            self.0.lock().unwrap().take();
            true
        }
    }

    #[test]
    fn entries_may_unregister_while_woken() {
        let queue = WaitQueue::new();
        let entry = Arc::new(SelfRemoving(Mutex::new(None)));
        *entry.0.lock().unwrap() = Some(queue.add(entry.clone()));

        queue.wake(PollMask::IN);

        assert!(!queue.is_active());
    }

    #[test]
    fn wakes_from_many_threads() {
        let queue = WaitQueue::new();
        let entry = counter(false, true);
        let _registration = queue.add(entry.clone());

        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..100 {
                        queue.wake(PollMask::OUT);
                    }
                });
            }
        });

        assert_eq!(hits(&entry), 800);
    }
}
