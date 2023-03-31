use dashmap::DashMap;
use fnv::FnvBuildHasher;
use std::sync::Arc;
use std::thread::{current, park, park_timeout, Thread};
use std::time::Duration;

/// A location in memory for a Waiter
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct NotifyLocation {
    /// The address of the Waiter location
    pub address: u32,
}

#[derive(Debug)]
struct NotifyWaiter {
    pub thread: Thread,
    pub notified: bool,
}
#[derive(Debug, Default)]
struct NotifyMap {
    pub map: DashMap<NotifyLocation, Vec<NotifyWaiter>, FnvBuildHasher>,
}

/// HashMap of Waiters for the Thread/Notify opcodes
#[derive(Debug)]
pub struct ThreadConditions {
    inner: Arc<NotifyMap>, // The Hasmap with the Notify for the Notify/wait opcodes
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
    pub fn do_wait(&mut self, dst: NotifyLocation, timeout: Option<Duration>) -> Option<u32> {
        // fetch the notifier
        if self.inner.map.len() >= 1 << 32 {
            return None;
        }
        self.inner
            .map
            .entry(dst)
            .or_insert_with(Vec::new)
            .push(NotifyWaiter {
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
        Some(ret)
    }

    /// Notify waiters from the wait list
    pub fn do_notify(&mut self, dst: NotifyLocation, count: u32) -> u32 {
        let mut count_token = 0u32;
        if let Some(mut v) = self.inner.map.get_mut(&dst) {
            for waiter in v.value_mut() {
                if count_token < count && !waiter.notified {
                    waiter.notified = true; // mark as was waiked up
                    waiter.thread.unpark(); // wakeup!
                    count_token += 1;
                }
            }
        }
        count_token
    }
}

impl Clone for ThreadConditions {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
#[test]
fn threadconditions_notify_nowaiters() {
    let mut conditions = ThreadConditions::new();
    let dst = NotifyLocation { address: 0 };
    let ret = conditions.do_notify(dst, 1);
    assert_eq!(ret, 0);
}

#[cfg(test)]
#[test]
fn threadconditions_notify_1waiter() {
    use std::thread;

    let mut conditions = ThreadConditions::new();
    let mut threadcond = conditions.clone();

    thread::spawn(move || {
        let dst = NotifyLocation { address: 0 };
        let ret = threadcond.do_wait(dst.clone(), None);
        assert_eq!(ret, Some(0));
    });
    thread::sleep(Duration::from_millis(1));
    let dst = NotifyLocation { address: 0 };
    let ret = conditions.do_notify(dst, 1);
    assert_eq!(ret, 1);
}

#[cfg(test)]
#[test]
fn threadconditions_notify_waiter_timeout() {
    use std::thread;

    let mut conditions = ThreadConditions::new();
    let mut threadcond = conditions.clone();

    thread::spawn(move || {
        let dst = NotifyLocation { address: 0 };
        let ret = threadcond.do_wait(dst.clone(), Some(Duration::from_millis(1)));
        assert_eq!(ret, Some(2));
    });
    thread::sleep(Duration::from_millis(10));
    let dst = NotifyLocation { address: 0 };
    let ret = conditions.do_notify(dst, 1);
    assert_eq!(ret, 0);
}

#[cfg(test)]
#[test]
fn threadconditions_notify_waiter_mismatch() {
    use std::thread;

    let mut conditions = ThreadConditions::new();
    let mut threadcond = conditions.clone();

    thread::spawn(move || {
        let dst = NotifyLocation { address: 8 };
        let ret = threadcond.do_wait(dst.clone(), Some(Duration::from_millis(10)));
        assert_eq!(ret, Some(2));
    });
    thread::sleep(Duration::from_millis(1));
    let dst = NotifyLocation { address: 0 };
    let ret = conditions.do_notify(dst, 1);
    assert_eq!(ret, 0);
    thread::sleep(Duration::from_millis(10));
}

#[cfg(test)]
#[test]
fn threadconditions_notify_2waiters() {
    use std::thread;

    let mut conditions = ThreadConditions::new();
    let mut threadcond = conditions.clone();
    let mut threadcond2 = conditions.clone();

    thread::spawn(move || {
        let dst = NotifyLocation { address: 0 };
        let ret = threadcond.do_wait(dst.clone(), None);
        assert_eq!(ret, Some(0));
    });
    thread::spawn(move || {
        let dst = NotifyLocation { address: 0 };
        let ret = threadcond2.do_wait(dst.clone(), None);
        assert_eq!(ret, Some(0));
    });
    thread::sleep(Duration::from_millis(1));
    let dst = NotifyLocation { address: 0 };
    let ret = conditions.do_notify(dst, 5);
    assert_eq!(ret, 2);
}
