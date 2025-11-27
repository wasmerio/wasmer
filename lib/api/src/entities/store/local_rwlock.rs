//! A single-threaded async-aware RwLock implementation.
//!
//! This module provides [`LocalRwLock`], which is similar to `async_lock::RwLock`
//! but optimized for single-threaded async runtimes. It avoids atomic operations
//! and is `!Send + !Sync`, making it more efficient when thread safety is not needed.
//!
//! Like `async_lock::RwLock`, it provides `read_rc()` and `write_rc()` methods
//! that allow callers to asynchronously wait for the lock to become available,
//! and return guards with `'static` lifetimes by holding an `Rc` to the lock.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::future::Future;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// The main lock type.
pub struct LocalRwLock<T> {
    inner: Rc<LocalRwLockInner<T>>,
}

struct LocalRwLockInner<T> {
    value: UnsafeCell<T>,
    state: Cell<LockState>,
    read_waiters: RefCell<Vec<Option<Waker>>>,
    write_waiters: RefCell<Vec<Option<Waker>>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum LockState {
    Unlocked,
    Reading(usize), // count of readers
    Writing,
}

impl<T> LocalRwLock<T> {
    /// Creates a new `LocalRwLock` with the given value.
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(LocalRwLockInner {
                value: UnsafeCell::new(value),
                state: Cell::new(LockState::Unlocked),
                read_waiters: RefCell::new(Vec::new()),
                write_waiters: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Acquires a read lock, waiting asynchronously if necessary.
    ///
    /// The returned guard holds an `Rc` clone, allowing it to have a `'static` lifetime.
    pub fn read(&self) -> ReadFuture<T> {
        ReadFuture {
            inner: self.inner.clone(),
            waiter_index: Cell::new(None),
        }
    }

    /// Acquires a write lock, waiting asynchronously if necessary.
    ///
    /// The returned guard holds an `Rc` clone, allowing it to have a `'static` lifetime.
    pub fn write(&self) -> WriteFuture<T> {
        WriteFuture {
            inner: self.inner.clone(),
            waiter_index: Cell::new(None),
        }
    }

    /// Attempts to acquire a read lock with a `'static` lifetime without waiting.
    pub fn try_read(&self) -> Option<LocalRwLockReadGuard<T>> {
        if self.inner.try_read() {
            Some(LocalRwLockReadGuard {
                inner: self.inner.clone(),
            })
        } else {
            None
        }
    }

    /// Attempts to acquire a write lock with a `'static` lifetime without waiting.
    pub fn try_write(&self) -> Option<LocalRwLockWriteGuard<T>> {
        if self.inner.try_write() {
            Some(LocalRwLockWriteGuard {
                inner: self.inner.clone(),
            })
        } else {
            None
        }
    }

    /// Attempts to consume the lock if there are no active or waiting
    /// readers or writers, and returns the inner value if successful.
    pub fn consume(self) -> Result<T, Self> {
        if self.inner.state.get() == LockState::Unlocked
            && self.inner.read_waiters.borrow().is_empty()
            && self.inner.write_waiters.borrow().is_empty()
        {
            match Rc::try_unwrap(self.inner) {
                Ok(inner) => Ok(inner.value.into_inner()),
                Err(rc) => Err(Self { inner: rc }),
            }
        } else {
            Err(self)
        }
    }
}

impl<T> Clone for LocalRwLock<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> LocalRwLockInner<T> {
    fn try_read(&self) -> bool {
        match self.state.get() {
            LockState::Unlocked => {
                self.state.set(LockState::Reading(1));
                true
            }
            LockState::Reading(n) => {
                self.state.set(LockState::Reading(n + 1));
                true
            }
            LockState::Writing => false,
        }
    }

    fn try_write(&self) -> bool {
        match self.state.get() {
            LockState::Unlocked => {
                self.state.set(LockState::Writing);
                true
            }
            _ => false,
        }
    }

    fn release_read(&self) {
        match self.state.get() {
            LockState::Reading(1) => {
                self.state.set(LockState::Unlocked);
                self.wake_waiters();
            }
            LockState::Reading(n) if n > 1 => {
                self.state.set(LockState::Reading(n - 1));
            }
            _ => panic!("LocalRwLock: release_read called but not in Reading state"),
        }
    }

    fn release_write(&self) {
        match self.state.get() {
            LockState::Writing => {
                self.state.set(LockState::Unlocked);
                self.wake_waiters();
            }
            _ => panic!("LocalRwLock: release_write called but not in Writing state"),
        }
    }

    fn wake_waiters(&self) {
        // Wake writers first (priority)
        let mut write_waiters = self.write_waiters.borrow_mut();
        let has_writers = write_waiters.iter().any(|w| w.is_some());

        if has_writers {
            // If there are waiting writers, only wake them (they need exclusive access)
            for waker_slot in write_waiters.drain(..) {
                if let Some(waker) = waker_slot {
                    waker.wake();
                }
            }
        } else {
            // No writers waiting, wake all readers (they can share the lock)
            drop(write_waiters); // Release borrow before borrowing read_waiters
            let mut read_waiters = self.read_waiters.borrow_mut();
            for waker in read_waiters.drain(..).flatten() {
                waker.wake();
            }
        }
    }

    /// Shared polling logic for all futures.
    /// Returns Poll::Ready(()) if the lock was acquired, Poll::Pending otherwise.
    fn poll_lock(
        &self,
        waiter_index: &Cell<Option<usize>>,
        cx: &mut Context<'_>,
        try_lock: impl FnOnce(&Self) -> bool,
        is_write: bool,
    ) -> Poll<()> {
        if try_lock(self) {
            // If we successfully acquired the lock, remove our waiter slot if we registered one
            if let Some(index) = waiter_index.get() {
                let waiters = if is_write {
                    &self.write_waiters
                } else {
                    &self.read_waiters
                };
                let mut waiters = waiters.borrow_mut();
                if index < waiters.len() {
                    waiters[index] = None;
                }
            }
            return Poll::Ready(());
        }

        // Register or update our waker in the appropriate waiters list
        let waiters = if is_write {
            &self.write_waiters
        } else {
            &self.read_waiters
        };
        let mut waiters = waiters.borrow_mut();

        if let Some(index) = waiter_index.get() {
            // We already have a slot, check if we need to update it
            if index < waiters.len() {
                if let Some(existing) = &waiters[index] {
                    if !existing.will_wake(cx.waker()) {
                        waiters[index] = Some(cx.waker().clone());
                    }
                } else {
                    waiters[index] = Some(cx.waker().clone());
                }
            } else {
                // Our slot was somehow removed, register a new one
                let new_index = waiters.len();
                waiters.push(Some(cx.waker().clone()));
                waiter_index.set(Some(new_index));
            }
        } else {
            // First time registering
            let index = waiters.len();
            waiters.push(Some(cx.waker().clone()));
            waiter_index.set(Some(index));
        }

        Poll::Pending
    }

    /// Cleanup waiter slot on drop
    fn cleanup_waiter(&self, waiter_index: &Cell<Option<usize>>, is_write: bool) {
        if let Some(index) = waiter_index.get() {
            let waiters = if is_write {
                &self.write_waiters
            } else {
                &self.read_waiters
            };
            let mut waiters = waiters.borrow_mut();
            if index < waiters.len() {
                waiters[index] = None;
            }
        }
    }
}

// Guards with 'static lifetime (Rc-like)

/// A read guard with a `'static` lifetime, holding an `Rc` to the lock.
pub struct LocalRwLockReadGuard<T> {
    inner: Rc<LocalRwLockInner<T>>,
}

impl<T> LocalRwLockReadGuard<T> {
    /// Rebuild a handle to the lock from this [`LocalReadGuardRc`].
    pub fn lock_handle(me: &Self) -> LocalRwLock<T> {
        LocalRwLock {
            inner: me.inner.clone(),
        }
    }
}

impl<T> Deref for LocalRwLockReadGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.inner.value.get() }
    }
}

impl<T> Drop for LocalRwLockReadGuard<T> {
    fn drop(&mut self) {
        self.inner.release_read();
    }
}

/// A write guard with a `'static` lifetime, holding an `Rc` to the lock.
pub struct LocalRwLockWriteGuard<T> {
    inner: Rc<LocalRwLockInner<T>>,
}

impl<T> LocalRwLockWriteGuard<T> {
    /// Rebuild a handle to the lock from this [`LocalWriteGuardRc`].
    pub fn lock_handle(me: &Self) -> LocalRwLock<T> {
        LocalRwLock {
            inner: me.inner.clone(),
        }
    }
}

impl<T> Deref for LocalRwLockWriteGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.inner.value.get() }
    }
}

impl<T> DerefMut for LocalRwLockWriteGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.inner.value.get() }
    }
}

impl<T> Drop for LocalRwLockWriteGuard<T> {
    fn drop(&mut self) {
        self.inner.release_write();
    }
}

// Futures

/// Future returned by `read_rc()`.
pub struct ReadFuture<T> {
    inner: Rc<LocalRwLockInner<T>>,
    waiter_index: Cell<Option<usize>>,
}

impl<T> Future for ReadFuture<T> {
    type Output = LocalRwLockReadGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_index, cx, |inner| inner.try_read(), false)
            .is_ready()
        {
            Poll::Ready(LocalRwLockReadGuard {
                inner: self.inner.clone(),
            })
        } else {
            Poll::Pending
        }
    }
}

impl<T> Drop for ReadFuture<T> {
    fn drop(&mut self) {
        self.inner.cleanup_waiter(&self.waiter_index, false);
    }
}

/// Future returned by `write_rc()`.
pub struct WriteFuture<T> {
    inner: Rc<LocalRwLockInner<T>>,
    waiter_index: Cell<Option<usize>>,
}

impl<T> Future for WriteFuture<T> {
    type Output = LocalRwLockWriteGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_index, cx, |inner| inner.try_write(), true)
            .is_ready()
        {
            Poll::Ready(LocalRwLockWriteGuard {
                inner: self.inner.clone(),
            })
        } else {
            Poll::Pending
        }
    }
}

impl<T> Drop for WriteFuture<T> {
    fn drop(&mut self) {
        self.inner.cleanup_waiter(&self.waiter_index, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_read_write() {
        let lock = LocalRwLock::new(42);

        // Can acquire read lock
        let guard = lock.try_read().unwrap();
        assert_eq!(*guard, 42);
        drop(guard);

        // Can acquire write lock
        let mut guard = lock.try_write().unwrap();
        *guard = 100;
        drop(guard);

        // Value was updated
        let guard = lock.try_read().unwrap();
        assert_eq!(*guard, 100);
    }

    #[test]
    fn test_multiple_readers() {
        let lock = LocalRwLock::new(42);

        let guard1 = lock.try_read().unwrap();
        let guard2 = lock.try_read().unwrap();
        let guard3 = lock.try_read().unwrap();

        assert_eq!(*guard1, 42);
        assert_eq!(*guard2, 42);
        assert_eq!(*guard3, 42);

        // Cannot acquire write lock while readers exist
        assert!(lock.try_write().is_none());

        drop(guard1);
        assert!(lock.try_write().is_none());

        drop(guard2);
        assert!(lock.try_write().is_none());

        drop(guard3);
        // Now we can acquire write lock
        assert!(lock.try_write().is_some());
    }

    #[test]
    fn test_exclusive_writer() {
        let lock = LocalRwLock::new(42);

        let guard = lock.try_write().unwrap();

        // Cannot acquire another write lock
        assert!(lock.try_write().is_none());

        // Cannot acquire read lock
        assert!(lock.try_read().is_none());

        drop(guard);

        // Now we can acquire locks again
        assert!(lock.try_read().is_some());
    }

    #[test]
    fn test_arc_guards() {
        let lock = LocalRwLock::new(42);

        // Try read_rc
        let guard = lock.try_read().unwrap();
        assert_eq!(*guard, 42);
        drop(guard);

        // Try write_rc
        let mut guard = lock.try_write().unwrap();
        *guard = 100;
        drop(guard);

        let guard = lock.try_read().unwrap();
        assert_eq!(*guard, 100);
    }

    #[test]
    fn test_writer_priority() {
        use std::sync::{Arc, Mutex};
        use std::task::{Poll, Wake};

        // Custom waker that tracks when it's woken
        struct TrackingWaker {
            name: &'static str,
            wake_order: Arc<Mutex<Vec<&'static str>>>,
        }

        impl Wake for TrackingWaker {
            fn wake(self: Arc<Self>) {
                self.wake_order.lock().unwrap().push(self.name);
            }
        }

        let lock = LocalRwLock::new(0);
        let wake_order = Arc::new(Mutex::new(Vec::new()));

        // Acquire write lock
        let write_guard = lock.try_write().unwrap();

        // Create read and write futures that will need to wait
        let mut read_future = Box::pin(lock.read());
        let mut write_future = Box::pin(lock.write());

        // Create tracking wakers
        let read_waker = Arc::new(TrackingWaker {
            name: "reader",
            wake_order: wake_order.clone(),
        })
        .into();
        let mut read_cx = std::task::Context::from_waker(&read_waker);

        let write_waker = Arc::new(TrackingWaker {
            name: "writer",
            wake_order: wake_order.clone(),
        })
        .into();
        let mut write_cx = std::task::Context::from_waker(&write_waker);

        // Poll both to register them as waiters
        assert!(matches!(
            write_future.as_mut().poll(&mut write_cx),
            Poll::Pending
        ));
        assert!(matches!(
            read_future.as_mut().poll(&mut read_cx),
            Poll::Pending
        ));

        // Verify they're in separate queues
        assert_eq!(
            lock.inner
                .write_waiters
                .borrow()
                .iter()
                .filter(|w| w.is_some())
                .count(),
            1
        );
        assert_eq!(
            lock.inner
                .read_waiters
                .borrow()
                .iter()
                .filter(|w| w.is_some())
                .count(),
            1
        );

        // No wakers called yet
        assert_eq!(wake_order.lock().unwrap().len(), 0);

        // Release the write lock - this should wake waiters
        drop(write_guard);

        // Verify ONLY writer was woken (not reader, since writer has priority)
        let order = wake_order.lock().unwrap();
        assert_eq!(
            order.len(),
            1,
            "Only writer should be woken when writers are waiting"
        );
        assert_eq!(order[0], "writer", "Writer should be woken, not reader");
    }

    #[test]
    fn test_readers_woken_when_no_writers() {
        use std::sync::{Arc, Mutex};
        use std::task::{Poll, Wake};

        // Custom waker that tracks when it's woken
        struct TrackingWaker {
            name: String,
            wake_order: Arc<Mutex<Vec<String>>>,
        }

        impl Wake for TrackingWaker {
            fn wake(self: Arc<Self>) {
                self.wake_order.lock().unwrap().push(self.name.clone());
            }
        }

        let lock = LocalRwLock::new(0);
        let wake_order = Arc::new(Mutex::new(Vec::new()));

        // Acquire write lock
        let write_guard = lock.try_write().unwrap();

        // Create multiple read futures (no writers)
        let mut read_future1 = Box::pin(lock.read());
        let mut read_future2 = Box::pin(lock.read());

        // Create tracking wakers
        let read_waker1 = Arc::new(TrackingWaker {
            name: "reader1".to_string(),
            wake_order: wake_order.clone(),
        })
        .into();
        let mut read_cx1 = std::task::Context::from_waker(&read_waker1);

        let read_waker2 = Arc::new(TrackingWaker {
            name: "reader2".to_string(),
            wake_order: wake_order.clone(),
        })
        .into();
        let mut read_cx2 = std::task::Context::from_waker(&read_waker2);

        // Poll both to register them as waiters
        assert!(matches!(
            read_future1.as_mut().poll(&mut read_cx1),
            Poll::Pending
        ));
        assert!(matches!(
            read_future2.as_mut().poll(&mut read_cx2),
            Poll::Pending
        ));

        // Verify they're in read queue
        assert_eq!(
            lock.inner
                .read_waiters
                .borrow()
                .iter()
                .filter(|w| w.is_some())
                .count(),
            2
        );
        assert_eq!(
            lock.inner
                .write_waiters
                .borrow()
                .iter()
                .filter(|w| w.is_some())
                .count(),
            0
        );

        // No wakers called yet
        assert_eq!(wake_order.lock().unwrap().len(), 0);

        // Release the write lock - this should wake all readers since no writers are waiting
        drop(write_guard);

        // Verify both readers were woken
        let order = wake_order.lock().unwrap();
        assert_eq!(
            order.len(),
            2,
            "Both readers should be woken when no writers are waiting"
        );
        assert!(order.contains(&"reader1".to_string()));
        assert!(order.contains(&"reader2".to_string()));
    }
}
