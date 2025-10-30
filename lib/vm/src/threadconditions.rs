use std::{
    sync::atomic::AtomicPtr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;
use fnv::FnvBuildHasher;
use parking_lot::{Condvar, Mutex};
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

/// Expected value for atomic waits
pub enum ExpectedValue {
    /// No expected value; this is used for native waits only.
    None,

    /// 32-bit expected value
    U32(u32),

    /// 64-bit expected value
    U64(u64),
}

/// A location in memory for a Waiter
#[derive(Clone, Copy, Debug)]
pub struct NotifyLocation {
    /// The address of the Waiter location
    pub address: u32,
    /// The base of the memory this address is relative to
    pub memory_base: *mut u8,
}

#[derive(Debug, Default)]
struct NotifyMap {
    /// If set to true, all waits will fail with an error.
    closed: AtomicBool,

    // For each wait address, we store a mutex and a condvar. The condvar is
    // used to handle sleeping and waking, while the mutex stores the
    // (manually-updated) number of waiters on that address. This lets us
    // know when there are no more waiters so we can clean up the map entry.
    // note that using a Weak here would be insufficient since it can't
    // clean up the map entries for us, only the mutexes/condvars.
    map: DashMap<u32, Arc<(Mutex<u32>, Condvar)>, FnvBuildHasher>,
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
    // to track the address of waiter. The key of the hashmap is based on the memory.
    // The actual waiting is implemented with a Condvar + Mutex pair. A Weak is stored
    // in the hashmap to at least delete the condvar and mutex when there are no
    // waiters for a given address. Map keys are currently not cleaned up.

    /// Add current thread to the waiter hash
    ///
    /// # Safety
    /// If `expected` is [`ExpectedValue::None`], no safety requirements.
    /// The notify location must have a valid base address that belongs to a memory,
    /// and the address must be a valid offset within that memory. The offset also
    /// must be properly aligned for the expected value type; either 4-byte aligned for
    /// [`ExpectedValue::U32`] or 8-byte aligned for [`ExpectedValue::U64`].
    pub unsafe fn do_wait(
        &mut self,
        dst: NotifyLocation,
        expected: ExpectedValue,
        timeout: Option<Duration>,
    ) -> Result<u32, WaiterError> {
        if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(WaiterError::AtomicsDisabled);
        }

        if self.inner.map.len() as u64 >= 1u64 << 32 {
            return Err(WaiterError::TooManyWaiters);
        }

        // Step 1: lock the map key, so we know no one else can get/create a
        // different Arc than the one we're getting/creating
        let entry = self.inner.map.entry(dst.address);
        let ref_mut = entry.or_default();
        let arc = ref_mut.clone();

        // Step 2: lock the mutex while still holding the map lock, so nobody
        // can delete the map key or make a new Arc
        let mut mutex_guard = arc.0.lock();

        // Step 3: unlock the map key, we don't need it anymore.
        drop(ref_mut);

        // Once we lock the mutex, we can check the expected value. A notifying
        // thread will have written an updated value to the address *before*
        // doing the notify call, and the call has to acquire the same lock we're
        // holding. This means we can't miss an update to the expected value that
        // would prevent us from sleeping.
        // This logic mirrors how the linux kernel's futex syscall works, so see
        // the documentation on that if I made zero sense here.

        // Safety: the function's safety contract ensures that the memory location is valid
        // and can be dereferenced.
        let should_sleep = match expected {
            ExpectedValue::None => true,
            ExpectedValue::U32(expected_val) => unsafe {
                let src = dst.memory_base.offset(dst.address as isize) as *mut u32;
                let atomic_src = AtomicPtr::new(src);
                let read_val = *atomic_src.load(Ordering::Acquire);
                read_val == expected_val
            },
            ExpectedValue::U64(expected_val) => unsafe {
                let src = dst.memory_base.offset(dst.address as isize) as *mut u64;
                let atomic_src = AtomicPtr::new(src);
                let read_val = *atomic_src.load(Ordering::Acquire);
                read_val == expected_val
            },
        };

        let ret = if should_sleep {
            *mutex_guard += 1;

            let ret = if let Some(timeout) = timeout {
                let timeout = arc.1.wait_for(&mut mutex_guard, timeout);
                if timeout.timed_out() {
                    2 // timeout
                } else {
                    0 // notified
                }
            } else {
                arc.1.wait(&mut mutex_guard);
                0
            };

            *mutex_guard -= 1;

            ret
        } else {
            1 // value mismatch
        };

        {
            // Note we use two sets of locks; one for the map itself, and one per
            // wait address. Locking order must stay consistent at all times: map
            // first, then mutex. So we have to drop the mutex guard here and then
            // reacquire it after locking the map key to avoid deadlocks.
            drop(mutex_guard);

            // Same as above, first lock the map key...
            let entry = self.inner.map.entry(dst.address);
            if let dashmap::Entry::Occupied(occupied) = entry {
                // ... then lock the mutex.
                let arc = occupied.get().clone();
                let mutex_guard = arc.0.lock();

                if *mutex_guard == 0 {
                    // No more waiters, remove the map entry.
                    occupied.remove();
                }
            }
        }

        Ok(ret)
    }

    /// Notify waiters from the wait list
    pub fn do_notify(&mut self, dst: u32, count: u32) -> u32 {
        let mut count_token = 0u32;
        if let Some(v) = self.inner.map.get(&dst) {
            let mutex_guard = v.0.lock();
            for _ in 0..count {
                if !v.1.notify_one() {
                    break;
                }
                count_token += 1;
            }
            drop(mutex_guard);
        }
        count_token
    }

    /// Wake all the waiters, *without* marking them as notified.
    ///
    /// Useful on shutdown to resume execution in all waiters.
    pub fn wake_all_atomic_waiters(&self) {
        for item in self.inner.map.iter_mut() {
            let arc = item.value();
            let _mutex_guard = arc.0.lock();
            arc.1.notify_all();
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
