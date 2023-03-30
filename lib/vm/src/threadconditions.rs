use std::collections::HashMap;
use std::sync::{Arc, LockResult, Mutex, MutexGuard};
use std::thread::{current, park, park_timeout, Thread};

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
struct NotifyLocation {
    pub address: u32,
}

#[derive(Debug)]
struct NotifyWaiter {
    pub thread: Thread,
    pub notified: bool,
}
#[derive(Debug, Default)]
struct NotifyMap {
    pub map: HashMap<NotifyLocation, Vec<NotifyWaiter>>,
}

/// HashMap of Waiters for the Thread/Notify opcodes
#[derive(Debug)]
pub struct ThreadConditions {
    inner: Arc<Mutex<NotifyMap>>, // The Hasmap with the Notify for the Notify/wait opcodes
}

impl ThreadConditions {
    /// Create a new ThreadConditions
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(NotifyMap::default())),
        }
    }

    fn lock_conditions(&mut self) -> LockResult<MutexGuard<NotifyMap>> {
        self.inner.lock()
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
    pub fn do_wait(&mut self, dst: u32, timeout: i64) -> u32 {
        // fetch the notifier
        let key = NotifyLocation { address: dst };
        let mut conds = self.lock_conditions().unwrap();
        if conds.map.len() > 1 << 32 {
            return 0xffff;
        }
        let v = conds.map.entry(key).or_insert_with(Vec::new);
        v.push(NotifyWaiter {
            thread: current(),
            notified: false,
        });
        drop(conds);
        if timeout < 0 {
            park();
        } else {
            park_timeout(std::time::Duration::from_nanos(timeout as u64));
        }
        let mut conds = self.lock_conditions().unwrap();
        let v = conds.map.get_mut(&key).unwrap();
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
        if v.is_empty() {
            conds.map.remove(&key);
        }
        ret
    }

    /// Notify waiters from the wait list
    pub fn do_notify(&mut self, dst: u32, count: u32) -> u32 {
        let key = NotifyLocation { address: dst };
        let mut conds = self.lock_conditions().unwrap();
        let mut cnt = 0u32;
        if let Some(v) = conds.map.get_mut(&key) {
            for waiter in v {
                if cnt < count && !waiter.notified {
                    waiter.notified = true; // mark as was waiked up
                    waiter.thread.unpark(); // wakeup!
                    cnt += 1;
                }
            }
        }
        cnt
    }
}

impl Clone for ThreadConditions {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
