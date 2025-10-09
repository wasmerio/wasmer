use dashmap::DashMap;
use fnv::FnvBuildHasher;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::{Thread, current, park, park_timeout};
use std::time::Duration;
use thiserror::Error;

/// Error that can occur during wait/notify calls.
// Non-exhaustive to allow for future variants without breaking changes!
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WaiterError {
    /// Wait/Notify is not implemented for this memory
    Unimplemented,
    /// To many waiter for an address
    TooManyWaiters,
    /// Atomic operations are disabled.
    AtomicsDisabled,
}

impl std::fmt::Display for WaiterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WaiterError")
    }
}

/// A location in memory for a Waiter
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct NotifyLocation {
    /// The address of the Waiter location
    pub address: u32,
}

#[derive(Debug)]
struct NotifyWaiter {
    thread: Thread,
    notified: bool,
}

#[derive(Debug, Default)]
struct NotifyMap {
    /// If set to true, all waits will fail with an error.
    closed: AtomicBool,
    map: DashMap<NotifyLocation, Vec<NotifyWaiter>, FnvBuildHasher>,
}

/// HashMap of Waiters for the Thread/Notify opcodes
#[derive(Debug)]
pub struct ThreadConditions {
    inner: Arc<NotifyMap>, // The Hasmap with the Notify for the Notify/wait opcodes
}

impl Clone for ThreadConditions {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ThreadConditions {
    /// Create a new ThreadConditions
    pub fn new() -> Self {
        Self {
            inner: Arc::new(NotifyMap::default()),
        }
    }

    // To implement Wait / Notify, a HasMap, behind a mutex, will be used
    // to track the address of waiter. The key of the hashmap is based on the memory
    // and waiter threads are "park"'d (with or without timeout)
    // Notify will wake the waiters by simply "unpark" the thread
    // as the Thread info is stored on the HashMap
    // once unparked, the waiter thread will remove it's mark on the HashMap
    // timeout / awake is tracked with a boolean in the HashMap
    // because `park_timeout` doesn't gives any information on why it returns

    /// Add current thread to the waiter hash
    pub fn do_wait(
        &mut self,
        dst: NotifyLocation,
        timeout: Option<Duration>,
    ) -> Result<u32, WaiterError> {
        if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(WaiterError::AtomicsDisabled);
        }

        // fetch the notifier
        if self.inner.map.len() as u64 >= 1u64 << 32 {
            return Err(WaiterError::TooManyWaiters);
        }
        self.inner.map.entry(dst).or_default().push(NotifyWaiter {
            thread: current(),
            notified: false,
        });
        if let Some(timeout) = timeout {
            park_timeout(timeout);
        } else {
            park();
        }
        let mut bindding = self.inner.map.get_mut(&dst).unwrap();
        let v = bindding.value_mut();
        let id = current().id();
        let mut ret = 0;
        v.retain(|cond| {
            if cond.thread.id() == id {
                ret = if cond.notified { 0 } else { 2 };
                false
            } else {
                true
            }
        });
        let empty = v.is_empty();
        drop(bindding);
        if empty {
            self.inner.map.remove(&dst);
        }
        Ok(ret)
    }

    /// Notify waiters from the wait list
    pub fn do_notify(&mut self, dst: NotifyLocation, count: u32) -> u32 {
        let mut count_token = 0u32;
        if let Some(mut v) = self.inner.map.get_mut(&dst) {
            for waiter in v.value_mut() {
                if count_token < count && !waiter.notified {
                    waiter.notified = true; // waiter was notified, not just an elapsed timeout
                    waiter.thread.unpark(); // wakeup!
                    count_token += 1;
                }
            }
        }
        count_token
    }

    /// Wake all the waiters, *without* marking them as notified.
    ///
    /// Useful on shutdown to resume execution in all waiters.
    pub fn wake_all_atomic_waiters(&self) {
        for mut item in self.inner.map.iter_mut() {
            for waiter in item.value_mut() {
                waiter.thread.unpark();
            }
        }
    }

    /// Disable the use of atomics, leading to all atomic waits failing with
    /// an error, which leads to a Webassembly trap.
    ///
    /// Useful for force-closing instances that keep waiting on atomics.
    pub fn disable_atomics(&self) {
        self.inner
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.wake_all_atomic_waiters();
    }

    /// Get a weak handle to this `ThreadConditions` instance.
    ///
    /// See [`ThreadConditionsHandle`] for more information.
    pub fn downgrade(&self) -> ThreadConditionsHandle {
        ThreadConditionsHandle {
            inner: Arc::downgrade(&self.inner),
        }
    }
}

/// A weak handle to a `ThreadConditions` instance, which does not prolong its
/// lifetime.
///
/// Internally holds a [`std::sync::Weak`] pointer.
pub struct ThreadConditionsHandle {
    inner: std::sync::Weak<NotifyMap>,
}

impl ThreadConditionsHandle {
    /// Attempt to upgrade this handle to a strong reference.
    ///
    /// Returns `None` if the original `ThreadConditions` instance has been dropped.
    pub fn upgrade(&self) -> Option<ThreadConditions> {
        self.inner.upgrade().map(|inner| ThreadConditions { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threadconditions_notify_nowaiters() {
        let mut conditions = ThreadConditions::new();
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn threadconditions_notify_1waiter() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(10));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 1);
    }

    #[test]
    fn threadconditions_notify_waiter_timeout() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond
                .do_wait(dst, Some(Duration::from_millis(1)))
                .unwrap();
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(50));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn threadconditions_notify_waiter_mismatch() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 8 };
            let ret = threadcond
                .do_wait(dst, Some(Duration::from_millis(10)))
                .unwrap();
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(1));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn threadconditions_notify_2waiters() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();
        let mut threadcond2 = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond2.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(20));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 5);
        assert_eq!(ret, 2);
    }
}
