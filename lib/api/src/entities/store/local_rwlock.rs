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
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

/// The main lock type.
pub struct LocalRwLock<T> {
    inner: Rc<LocalRwLockInner<T>>,
}

pub(crate) struct WeakLocalRwLock<T> {
    inner: Weak<LocalRwLockInner<T>>,
}

impl<T> Clone for WeakLocalRwLock<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> WeakLocalRwLock<T> {
    pub(crate) fn upgrade(&self) -> Option<LocalRwLock<T>> {
        self.inner.upgrade().map(|inner| LocalRwLock { inner })
    }
}

struct LocalRwLockInner<T> {
    value: UnsafeCell<T>,
    state: Cell<LockState>,
    read_waiters: RefCell<Waiters>,
    write_waiters: RefCell<Waiters>,
}

#[derive(Clone, Copy)]
struct WaiterKey {
    generation: u64,
    index: usize,
}

#[derive(Default)]
struct Waiters {
    generation: u64,
    entries: Vec<Option<Waker>>,
}

impl Waiters {
    fn contains(&self, key: WaiterKey) -> bool {
        self.generation == key.generation && key.index < self.entries.len()
    }

    fn take(&mut self, key: WaiterKey) -> Option<Waker> {
        if self.contains(key) {
            self.entries[key.index].take()
        } else {
            None
        }
    }

    fn take_all(&mut self) -> Vec<Option<Waker>> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("LocalRwLock waiter generation exhausted");
        std::mem::take(&mut self.entries)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum LockState {
    Unlocked,
    Reading(usize), // count of readers
    Writing,
}

impl<T> LocalRwLock<T> {
    pub(crate) fn downgrade(&self) -> WeakLocalRwLock<T> {
        WeakLocalRwLock {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// Creates a new `LocalRwLock` with the given value.
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(LocalRwLockInner {
                value: UnsafeCell::new(value),
                state: Cell::new(LockState::Unlocked),
                read_waiters: RefCell::default(),
                write_waiters: RefCell::default(),
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
            && self
                .inner
                .read_waiters
                .borrow()
                .entries
                .iter()
                .all(Option::is_none)
            && self
                .inner
                .write_waiters
                .borrow()
                .entries
                .iter()
                .all(Option::is_none)
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
        // Invalidate old slot keys before arbitrary wake code can register new waiters.
        let write_waiters = self.write_waiters.borrow_mut().take_all();
        if write_waiters.iter().any(Option::is_some) {
            for waker in write_waiters.into_iter().flatten() {
                waker.wake();
            }
        } else {
            let read_waiters = self.read_waiters.borrow_mut().take_all();
            for waker in read_waiters.into_iter().flatten() {
                waker.wake();
            }
        }
    }

    /// Shared polling logic for all futures.
    /// Returns Poll::Ready(()) if the lock was acquired, Poll::Pending otherwise.
    fn poll_lock(
        &self,
        waiter_index: &Cell<Option<WaiterKey>>,
        cx: &mut Context<'_>,
        try_lock: impl Fn(&Self) -> bool,
        is_write: bool,
    ) -> Poll<()> {
        if try_lock(self) {
            // The caller constructs its guard before dropping a previous Waker.
            return Poll::Ready(());
        }

        let waiters = if is_write {
            &self.write_waiters
        } else {
            &self.read_waiters
        };
        {
            let waiters = waiters.borrow();
            if let Some(key) = waiter_index.get()
                && waiters.contains(key)
                && waiters.entries[key.index]
                    .as_ref()
                    .is_some_and(|existing| existing.will_wake(cx.waker()))
            {
                return Poll::Pending;
            }
        }

        // Clone and destroy user Wakers outside the queue borrow. Either callback
        // can release the owner, so check acquisition again after registration.
        let waker = cx.waker().clone();
        let previous = {
            let mut waiters = waiters.borrow_mut();
            if let Some(key) = waiter_index.get()
                && waiters.contains(key)
            {
                waiters.entries[key.index].replace(waker)
            } else {
                let key = WaiterKey {
                    generation: waiters.generation,
                    index: waiters.entries.len(),
                };
                waiters.entries.push(Some(waker));
                waiter_index.set(Some(key));
                None
            }
        };
        drop(previous);
        if try_lock(self) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    /// Cleanup waiter slot on drop
    fn cleanup_waiter(&self, waiter_index: &Cell<Option<WaiterKey>>, is_write: bool) {
        if let Some(key) = waiter_index.take() {
            let waiters = if is_write {
                &self.write_waiters
            } else {
                &self.read_waiters
            };
            let previous = waiters.borrow_mut().take(key);
            drop(previous);
        }
    }
}

// Guards with 'static lifetime (Rc-like)

/// A read guard with a `'static` lifetime, holding an `Rc` to the lock.
pub struct LocalRwLockReadGuard<T> {
    inner: Rc<LocalRwLockInner<T>>,
}

impl<T> LocalRwLockReadGuard<T> {
    /// Rebuild a handle to the lock from this [`LocalRwLockReadGuard`].
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
    /// Rebuild a handle to the lock from this [`LocalRwLockWriteGuard`].
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
    waiter_index: Cell<Option<WaiterKey>>,
}

impl<T> Future for ReadFuture<T> {
    type Output = LocalRwLockReadGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_index, cx, |inner| inner.try_read(), false)
            .is_ready()
        {
            let guard = LocalRwLockReadGuard {
                inner: self.inner.clone(),
            };
            self.inner.cleanup_waiter(&self.waiter_index, false);
            Poll::Ready(guard)
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
    waiter_index: Cell<Option<WaiterKey>>,
}

impl<T> Future for WriteFuture<T> {
    type Output = LocalRwLockWriteGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_index, cx, |inner| inner.try_write(), true)
            .is_ready()
        {
            let guard = LocalRwLockWriteGuard {
                inner: self.inner.clone(),
            };
            self.inner.cleanup_waiter(&self.waiter_index, true);
            Poll::Ready(guard)
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
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{RawWaker, RawWakerVTable, Wake},
    };

    thread_local! {
        static ON_WAKE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
        static ON_CLONE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    struct ReentrantWake;
    impl Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            let callback = ON_WAKE.with(|callback| callback.borrow_mut().take());
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    struct DropPanicWake;
    // This Waker's destructor is the operation exercised by the regression.
    #[allow(clippy::manual_noop_waker)]
    impl Wake for DropPanicWake {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for DropPanicWake {
        fn drop(&mut self) {
            panic!("registered Waker destructor");
        }
    }

    struct CountWake(Arc<AtomicUsize>);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn clone_callback_waker() -> Waker {
        unsafe fn clone(data: *const ()) -> RawWaker {
            let owner = std::mem::ManuallyDrop::new(unsafe { Arc::<()>::from_raw(data) });
            let cloned = Arc::clone(&owner);
            let callback = ON_CLONE.with(|callback| callback.borrow_mut().take());
            if let Some(callback) = callback {
                callback();
            }
            RawWaker::new(Arc::into_raw(cloned), &VTABLE)
        }
        unsafe fn wake(data: *const ()) {
            drop(unsafe { Arc::<()>::from_raw(data) });
        }
        unsafe fn wake_by_ref(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, wake);
        // The data is an Arc, including on foreign threads. The optional test
        // callback belongs only to the thread performing a clone.
        unsafe { Waker::from_raw(RawWaker::new(Arc::into_raw(Arc::new(())), &VTABLE)) }
    }

    #[test]
    fn panicking_waiter_drop_releases_the_newly_acquired_read_lock() {
        let lock = LocalRwLock::new(42);
        let owner = lock.try_write().unwrap();
        let mut reader = Box::pin(lock.read());
        let waker = Waker::from(Arc::new(DropPanicWake));
        assert!(
            reader
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        // Only the registered queue entry now owns the panicking Waker.
        drop(waker);
        let mut writer = Box::pin(lock.write());
        assert!(
            writer
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        // Writer priority leaves the reader's Waker in its queue.
        drop(owner);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                reader
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
            }))
            .is_err()
        );
        assert!(
            lock.try_write().is_some(),
            "a panicking Waker must not leak a reader count"
        );
    }

    #[test]
    fn a_wake_callback_can_poll_and_release_its_original_waiter() {
        let lock = LocalRwLock::new(42);
        let owner = lock.try_write().unwrap();
        let waiter = Rc::new(RefCell::new(Box::pin(lock.write())));
        let waker = Waker::from(Arc::new(ReentrantWake));
        assert!(
            waiter
                .borrow_mut()
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let called = Rc::new(Cell::new(false));
        ON_WAKE.with(|callback| {
            let waiter = waiter.clone();
            let called = called.clone();
            *callback.borrow_mut() = Some(Box::new(move || {
                let Poll::Ready(guard) = waiter
                    .borrow_mut()
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                else {
                    panic!("the owner released the lock before waking its waiter");
                };
                assert_eq!(*guard, 42);
                called.set(true);
                drop(guard);
            }));
        });
        drop(owner);
        assert!(called.get());
        assert!(lock.try_write().is_some());
    }

    #[test]
    fn an_old_waiter_cannot_remove_a_new_waiter_from_a_reused_slot() {
        let lock = LocalRwLock::new(42);
        let owner = lock.try_write().unwrap();
        let mut first = Box::pin(lock.write());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(owner);
        let owner = lock.try_write().unwrap();
        let mut second = Box::pin(lock.write());
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(CountWake(wakes.clone())));
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(first);
        drop(owner);
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_ready()
        );
    }

    #[test]
    fn cloning_a_waiter_can_release_the_owner_without_losing_progress() {
        let lock = LocalRwLock::new(42);
        let owner = Rc::new(RefCell::new(Some(lock.try_write().unwrap())));
        ON_CLONE.with(|callback| {
            let owner = owner.clone();
            *callback.borrow_mut() = Some(Box::new(move || drop(owner.borrow_mut().take())));
        });
        let mut waiter = Box::pin(lock.write());
        let waker = clone_callback_waker();
        let Poll::Ready(guard) = waiter.as_mut().poll(&mut Context::from_waker(&waker)) else {
            panic!("reentrant release during registration must not lose its wake");
        };
        assert!(owner.borrow().is_none());
        assert_eq!(*guard, 42);
        drop(guard);
        assert!(lock.try_read().is_some());
    }

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
                .entries
                .iter()
                .filter(|w| w.is_some())
                .count(),
            1
        );
        assert_eq!(
            lock.inner
                .read_waiters
                .borrow()
                .entries
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
                .entries
                .iter()
                .filter(|w| w.is_some())
                .count(),
            2
        );
        assert_eq!(
            lock.inner
                .write_waiters
                .borrow()
                .entries
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
