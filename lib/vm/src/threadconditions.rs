use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use dashmap::DashMap;
use fnv::FnvBuildHasher;
use parking_lot::{Condvar, Mutex};
use thiserror::Error;
use wasmer_types::StoreId;

#[cfg(feature = "experimental-host-interrupt")]
use crate::interrupt_registry::{self, InterruptWaitWaker};

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
    /// The store executing this wait was interrupted by its host.
    Interrupted,
}

const WAITER_WAITING: u8 = 0;
const WAITER_NOTIFIED: u8 = 1;
#[cfg(feature = "experimental-host-interrupt")]
const WAITER_INTERRUPTED: u8 = 2;

#[derive(Debug, Default)]
struct AtomicWaiter {
    condvar: Condvar,
    outcome: AtomicU8,
}

#[derive(Debug, Default)]
struct WaitState {
    waiters: Vec<Arc<AtomicWaiter>>,
}

#[cfg(feature = "experimental-host-interrupt")]
struct AtomicWaitInterruptWaker {
    state: Arc<Mutex<WaitState>>,
    waiter: Arc<AtomicWaiter>,
}

#[cfg(feature = "experimental-host-interrupt")]
impl InterruptWaitWaker for AtomicWaitInterruptWaker {
    fn wake(&self) {
        self.waiter
            .outcome
            .store(WAITER_INTERRUPTED, Ordering::Release);
        let _guard = self.state.lock();
        self.waiter.condvar.notify_one();
    }
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
    map: DashMap<u32, Arc<Mutex<WaitState>>, FnvBuildHasher>,
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
        // The hook is optimized away outside tests. It lets the regression test
        // deterministically close the memory after the fast check below.
        unsafe { self.do_wait_with_registration_hook(dst, expected, timeout, None, || {}, || {}) }
    }

    /// Wait while participating in a running store invocation.
    ///
    /// A store interrupt wakes this wait cooperatively so the native
    /// synchronization can run on the host stack without abandoning Rust lock
    /// guards from the signal handler.
    ///
    /// # Safety
    ///
    /// The destination must satisfy the same validity and alignment
    /// requirements as [`Self::do_wait`].
    pub unsafe fn do_wait_interruptible(
        &mut self,
        dst: NotifyLocation,
        expected: ExpectedValue,
        timeout: Option<Duration>,
        store_id: StoreId,
    ) -> Result<u32, WaiterError> {
        unsafe {
            self.do_wait_with_registration_hook(
                dst,
                expected,
                timeout,
                Some(store_id),
                || {},
                || {},
            )
        }
    }

    unsafe fn do_wait_with_registration_hook(
        &mut self,
        dst: NotifyLocation,
        expected: ExpectedValue,
        timeout: Option<Duration>,
        _store_id: Option<StoreId>,
        before_registration: impl FnOnce(),
        after_interrupt_registration: impl FnOnce(),
    ) -> Result<u32, WaiterError> {
        if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(WaiterError::AtomicsDisabled);
        }

        before_registration();

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
        let mut mutex_guard = arc.lock();

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
                let read_val = AtomicU32::from_ptr(src).load(Ordering::Acquire);
                read_val == expected_val
            },
            ExpectedValue::U64(expected_val) => unsafe {
                let src = dst.memory_base.offset(dst.address as isize) as *mut u64;
                let read_val = AtomicU64::from_ptr(src).load(Ordering::Acquire);
                read_val == expected_val
            },
        };

        // Register while holding the address mutex. An interrupt either sees
        // the registration and wakes after condvar wait atomically releases
        // this mutex, or marks the store interrupted before registration and
        // makes us skip the wait. This closes the usual lost-wakeup window.
        let waiter = Arc::new(AtomicWaiter::default());
        if should_sleep {
            mutex_guard.waiters.push(waiter.clone());
        }
        #[cfg(feature = "experimental-host-interrupt")]
        let mut interrupted_before_wait = false;
        #[cfg(feature = "experimental-host-interrupt")]
        let interrupt_guard = if should_sleep {
            _store_id.and_then(|store_id| {
                let guard = interrupt_registry::register_wait(
                    store_id,
                    Arc::new(AtomicWaitInterruptWaker {
                        state: arc.clone(),
                        waiter: waiter.clone(),
                    }),
                );
                interrupted_before_wait = guard.is_none();
                guard
            })
        } else {
            None
        };
        #[cfg(not(feature = "experimental-host-interrupt"))]
        let interrupted_before_wait = false;
        #[cfg(not(feature = "experimental-host-interrupt"))]
        let interrupt_guard: Option<()> = None;

        after_interrupt_registration();

        // Closing and walking the map can finish between the fast check and
        // insertion above. Recheck under the address mutex: either shutdown
        // already happened, or its notifier must acquire this mutex after the
        // condvar has atomically registered the waiter and released the lock.
        let ret = if interrupted_before_wait {
            Err(WaiterError::Interrupted)
        } else if self.inner.closed.load(Ordering::Acquire) {
            Err(WaiterError::AtomicsDisabled)
        } else if should_sleep {
            let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
            loop {
                #[cfg(feature = "experimental-host-interrupt")]
                let interrupted = waiter.outcome.load(Ordering::Acquire) == WAITER_INTERRUPTED
                    || _store_id.is_some_and(interrupt_registry::is_interrupted);
                #[cfg(not(feature = "experimental-host-interrupt"))]
                let interrupted = false;

                if interrupted {
                    break Err(WaiterError::Interrupted);
                }
                if self.inner.closed.load(Ordering::Acquire) {
                    break Err(WaiterError::AtomicsDisabled);
                }
                if waiter.outcome.load(Ordering::Acquire) == WAITER_NOTIFIED {
                    break Ok(0);
                }

                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        break Ok(2);
                    }
                    waiter.condvar.wait_until(&mut mutex_guard, deadline);
                } else {
                    waiter.condvar.wait(&mut mutex_guard);
                }
            }
        } else {
            Ok(1) // value mismatch
        };

        #[cfg(feature = "experimental-host-interrupt")]
        drop(interrupt_guard);
        #[cfg(not(feature = "experimental-host-interrupt"))]
        let _ = interrupt_guard;

        if should_sleep {
            mutex_guard
                .waiters
                .retain(|registered| !Arc::ptr_eq(registered, &waiter));
        }

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
                let mutex_guard = arc.lock();

                if mutex_guard.waiters.is_empty() {
                    // No more waiters, remove the map entry.
                    occupied.remove();
                }
            }
        }

        ret
    }

    /// Notify waiters from the wait list
    pub fn do_notify(&mut self, dst: u32, count: u32) -> u32 {
        let mut count_token = 0u32;
        if let Some(v) = self.inner.map.get(&dst) {
            let state = v.lock();
            for waiter in &state.waiters {
                if count_token == count {
                    break;
                }
                if waiter
                    .outcome
                    .compare_exchange(
                        WAITER_WAITING,
                        WAITER_NOTIFIED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    waiter.condvar.notify_one();
                    count_token += 1;
                }
            }
        }
        count_token
    }

    /// Wake all waiters and let them resume as notified.
    ///
    /// Shutdown marks the conditions closed before calling this, so shutdown
    /// waiters still return [`WaiterError::AtomicsDisabled`].
    pub fn wake_all_atomic_waiters(&self) {
        for item in self.inner.map.iter_mut() {
            let state = item.value().lock();
            for waiter in &state.waiters {
                if waiter
                    .outcome
                    .compare_exchange(
                        WAITER_WAITING,
                        WAITER_NOTIFIED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    waiter.condvar.notify_one();
                }
            }
        }
    }

    /// Disable the use of atomics, leading to all atomic waits failing with
    /// an error, which leads to a Webassembly trap.
    ///
    /// NOTE: will also wake up all current waiters.
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
    fn disable_atomics_between_initial_check_and_registration() {
        let conditions = ThreadConditions::new();
        let mut waiter = conditions.clone();
        let closer = conditions.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = unsafe {
                waiter.do_wait_with_registration_hook(
                    NotifyLocation {
                        address: 0,
                        memory_base: std::ptr::null_mut(),
                    },
                    ExpectedValue::None,
                    Some(Duration::from_secs(5)),
                    None,
                    || closer.disable_atomics(),
                    || {},
                )
            };
            done_tx.send(result).unwrap();
        });

        let result = done_rx.recv_timeout(Duration::from_secs(2));
        // Release a regressed waiter before failing, so this test cannot strand
        // a native worker. Its map registration is visible by this point.
        if result.is_err() {
            conditions.wake_all_atomic_waiters();
        }
        worker.join().unwrap();
        assert!(matches!(result.unwrap(), Err(WaiterError::AtomicsDisabled)));
        assert!(conditions.inner.map.is_empty());
    }

    #[cfg(feature = "experimental-host-interrupt")]
    #[test]
    fn store_interrupt_after_wait_registration_cannot_be_lost_before_park() {
        crate::init_traps();
        let mut conditions = ThreadConditions::new();
        let store_id = StoreId::default();
        let (registered_tx, registered_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _install = crate::interrupt_registry::install(store_id).unwrap();
            let result = unsafe {
                conditions.do_wait_with_registration_hook(
                    NotifyLocation {
                        address: 0,
                        memory_base: std::ptr::null_mut(),
                    },
                    ExpectedValue::None,
                    None,
                    Some(store_id),
                    || {},
                    || {
                        registered_tx.send(()).unwrap();
                        while !crate::interrupt_registry::is_interrupted(store_id) {
                            std::thread::yield_now();
                        }
                    },
                )
            };
            done_tx.send(result).unwrap();
        });

        registered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let interrupt = std::thread::spawn(move || {
            crate::interrupt_registry::interrupt(store_id).unwrap();
        });
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(WaiterError::Interrupted)
        ));
        interrupt.join().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn disable_atomics_wakes_and_removes_registered_waiter() {
        let conditions = ThreadConditions::new();
        let mut waiter = conditions.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = unsafe {
                waiter.do_wait(
                    NotifyLocation {
                        address: 0,
                        memory_base: std::ptr::null_mut(),
                    },
                    ExpectedValue::None,
                    Some(Duration::from_secs(5)),
                )
            };
            done_tx.send(result).unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if conditions
                .inner
                .map
                .get(&0)
                .is_some_and(|entry| entry.lock().waiters.len() == 1)
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        conditions.disable_atomics();
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        worker.join().unwrap();
        assert!(matches!(result.unwrap(), Err(WaiterError::AtomicsDisabled)));
        assert!(conditions.inner.map.is_empty());
    }

    #[test]
    fn threadconditions_notify_nowaiters() {
        let mut conditions = ThreadConditions::new();
        let ret = conditions.do_notify(0, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn notification_cannot_be_consumed_by_a_late_waiter() {
        fn spawn(
            mut conditions: ThreadConditions,
        ) -> (
            std::sync::mpsc::Receiver<Result<u32, WaiterError>>,
            std::thread::JoinHandle<()>,
        ) {
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = unsafe {
                    conditions.do_wait(
                        NotifyLocation {
                            address: 0,
                            memory_base: std::ptr::null_mut(),
                        },
                        ExpectedValue::None,
                        None,
                    )
                };
                tx.send(result).unwrap();
            });
            (rx, worker)
        }

        let mut conditions = ThreadConditions::new();
        let (first_rx, first) = spawn(conditions.clone());
        while conditions
            .inner
            .map
            .get(&0)
            .is_none_or(|state| state.lock().waiters.len() != 1)
        {
            std::thread::yield_now();
        }
        assert_eq!(conditions.do_notify(0, 1), 1);

        let (late_rx, late) = spawn(conditions.clone());
        assert_eq!(
            first_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            0
        );
        assert!(late_rx.recv_timeout(Duration::from_millis(50)).is_err());
        let deadline = Instant::now() + Duration::from_secs(2);
        while conditions.do_notify(0, 1) == 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            late_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            0
        );
        first.join().unwrap();
        late.join().unwrap();
    }

    #[cfg(feature = "experimental-host-interrupt")]
    #[test]
    fn targeted_interrupt_does_not_consume_another_waiters_notification() {
        let conditions = ThreadConditions::new();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..2 {
            let mut waiter = conditions.clone();
            let done_tx = done_tx.clone();
            workers.push(std::thread::spawn(move || {
                let result = unsafe {
                    waiter.do_wait(
                        NotifyLocation {
                            address: 0,
                            memory_base: std::ptr::null_mut(),
                        },
                        ExpectedValue::None,
                        None,
                    )
                };
                done_tx.send(result).unwrap();
            }));
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        let (state, interrupted_waiter) = loop {
            if let Some(state) = conditions.inner.map.get(&0) {
                let guard = state.lock();
                if guard.waiters.len() == 2 {
                    let state = Arc::clone(state.value());
                    let waiter = guard.waiters[0].clone();
                    drop(guard);
                    break (state, waiter);
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        InterruptWaitWaker::wake(&AtomicWaitInterruptWaker {
            state,
            waiter: interrupted_waiter,
        });
        let mut notifier = conditions.clone();
        assert_eq!(notifier.do_notify(0, 1), 1);

        let first = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(&first, Err(WaiterError::Interrupted))
                || matches!(&second, Err(WaiterError::Interrupted))
        );
        assert!(matches!(&first, Ok(0)) || matches!(&second, Ok(0)));
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn overflowing_timeout_is_treated_as_unbounded() {
        let mut conditions = ThreadConditions::new();
        let mut waiter = conditions.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = unsafe {
                waiter.do_wait(
                    NotifyLocation {
                        address: 0,
                        memory_base: std::ptr::null_mut(),
                    },
                    ExpectedValue::None,
                    Some(Duration::MAX),
                )
            };
            done_tx.send(result).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while conditions.do_notify(0, 1) == 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            0
        );
        worker.join().unwrap();
    }

    #[test]
    fn threadconditions_notify_1waiter() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation {
                address: 0,
                memory_base: std::ptr::null_mut(),
            };
            let ret = unsafe { threadcond.do_wait(dst, ExpectedValue::None, None) }.unwrap();
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(10));
        let ret = conditions.do_notify(0, 1);
        assert_eq!(ret, 1);
    }

    #[test]
    fn threadconditions_notify_waiter_timeout() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation {
                address: 0,
                memory_base: std::ptr::null_mut(),
            };
            let ret = unsafe {
                threadcond
                    .do_wait(dst, ExpectedValue::None, Some(Duration::from_millis(1)))
                    .unwrap()
            };
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(50));
        let ret = conditions.do_notify(0, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn threadconditions_notify_waiter_mismatch() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation {
                address: 8,
                memory_base: std::ptr::null_mut(),
            };
            let ret = unsafe {
                threadcond
                    .do_wait(dst, ExpectedValue::None, Some(Duration::from_millis(10)))
                    .unwrap()
            };
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(1));
        let ret = conditions.do_notify(0, 1);
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
            let dst = NotifyLocation {
                address: 0,
                memory_base: std::ptr::null_mut(),
            };
            let ret = unsafe { threadcond.do_wait(dst, ExpectedValue::None, None).unwrap() };
            assert_eq!(ret, 0);
        });
        thread::spawn(move || {
            let dst = NotifyLocation {
                address: 0,
                memory_base: std::ptr::null_mut(),
            };
            let ret = unsafe { threadcond2.do_wait(dst, ExpectedValue::None, None).unwrap() };
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(20));
        let ret = conditions.do_notify(0, 5);
        assert_eq!(ret, 2);
    }

    #[test]
    fn threadconditions_value_mismatch() {
        let mut conditions = ThreadConditions::new();
        let mut data: u32 = 42;
        let dst = NotifyLocation {
            address: 0,
            memory_base: (&mut data as *mut u32) as *mut u8,
        };
        let ret = unsafe {
            conditions
                .do_wait(dst, ExpectedValue::U32(85), Some(Duration::from_millis(10)))
                .unwrap()
        };
        assert_eq!(ret, 1);
    }
}
