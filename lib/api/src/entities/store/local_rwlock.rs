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
    read_waiters: Waiters,
    write_waiters: Waiters,
    /// The key of the next future that starts waiting, for either list.
    next_waiter_key: Cell<u64>,
}

/// The wakers of the futures waiting for the lock, each under a key that its
/// future keeps for as long as it waits.
///
/// Keys are never reused. Waking the waiters empties the list while their
/// futures keep their keys, and a future that is woken but finds the lock
/// taken again registers once more under its own key; with positions for
/// keys, it would replace or remove the waker of a future that started
/// waiting in the meantime, which would then never be woken.
#[derive(Default)]
struct Waiters(RefCell<Vec<(u64, Waker)>>);

impl Waiters {
    fn register(&self, key: u64, waker: &Waker) {
        let mut waiters = self.0.borrow_mut();
        match waiters.iter_mut().find(|(k, _)| *k == key) {
            Some((_, existing)) => {
                if !existing.will_wake(waker) {
                    *existing = waker.clone();
                }
            }
            None => waiters.push((key, waker.clone())),
        }
    }

    fn remove(&self, key: u64) {
        let mut waiters = self.0.borrow_mut();
        if let Some(i) = waiters.iter().position(|(k, _)| *k == key) {
            waiters.remove(i);
        }
    }

    fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Wakes every waiter and empties the list.
    fn wake_all(&self) {
        // Taken out first: a waker may poll its future, which registers again.
        let waiters = std::mem::take(&mut *self.0.borrow_mut());
        for (_, waker) in waiters {
            waker.wake();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.borrow().len()
    }
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
                read_waiters: Waiters::default(),
                write_waiters: Waiters::default(),
                next_waiter_key: Cell::new(0),
            }),
        }
    }

    /// Acquires a read lock, waiting asynchronously if necessary.
    ///
    /// The returned guard holds an `Rc` clone, allowing it to have a `'static` lifetime.
    pub fn read(&self) -> ReadFuture<T> {
        ReadFuture {
            inner: self.inner.clone(),
            waiter_key: Cell::new(None),
        }
    }

    /// Acquires a write lock, waiting asynchronously if necessary.
    ///
    /// The returned guard holds an `Rc` clone, allowing it to have a `'static` lifetime.
    pub fn write(&self) -> WriteFuture<T> {
        WriteFuture {
            inner: self.inner.clone(),
            waiter_key: Cell::new(None),
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
        // A waiter that gave up has removed its waker.
        if self.inner.state.get() == LockState::Unlocked
            && self.inner.read_waiters.is_empty()
            && self.inner.write_waiters.is_empty()
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
        // Wake writers first (priority): if there are waiting writers, only
        // wake them (they need exclusive access). Otherwise wake all readers
        // (they can share the lock).
        if !self.write_waiters.is_empty() {
            self.write_waiters.wake_all();
        } else {
            self.read_waiters.wake_all();
        }
    }

    fn waiters(&self, is_write: bool) -> &Waiters {
        if is_write {
            &self.write_waiters
        } else {
            &self.read_waiters
        }
    }

    /// Shared polling logic for all futures.
    /// Returns Poll::Ready(()) if the lock was acquired, Poll::Pending otherwise.
    fn poll_lock(
        &self,
        waiter_key: &Cell<Option<u64>>,
        cx: &mut Context<'_>,
        try_lock: impl FnOnce(&Self) -> bool,
        is_write: bool,
    ) -> Poll<()> {
        if try_lock(self) {
            // If we successfully acquired the lock, remove our waker if we registered one
            if let Some(key) = waiter_key.take() {
                self.waiters(is_write).remove(key);
            }
            return Poll::Ready(());
        }

        // Register or update our waker under our key, which a future gets
        // the first time it has to wait
        let key = waiter_key.get().unwrap_or_else(|| {
            let key = self.next_waiter_key.get();
            self.next_waiter_key.set(key + 1);
            waiter_key.set(Some(key));
            key
        });
        self.waiters(is_write).register(key, cx.waker());

        Poll::Pending
    }

    /// Cleanup waiter slot on drop
    fn cleanup_waiter(&self, waiter_key: &Cell<Option<u64>>, is_write: bool) {
        if let Some(key) = waiter_key.get() {
            self.waiters(is_write).remove(key);
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
    waiter_key: Cell<Option<u64>>,
}

impl<T> Future for ReadFuture<T> {
    type Output = LocalRwLockReadGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_key, cx, |inner| inner.try_read(), false)
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
        self.inner.cleanup_waiter(&self.waiter_key, false);
    }
}

/// Future returned by `write_rc()`.
pub struct WriteFuture<T> {
    inner: Rc<LocalRwLockInner<T>>,
    waiter_key: Cell<Option<u64>>,
}

impl<T> Future for WriteFuture<T> {
    type Output = LocalRwLockWriteGuard<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self
            .inner
            .poll_lock(&self.waiter_key, cx, |inner| inner.try_write(), true)
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
        self.inner.cleanup_waiter(&self.waiter_key, true);
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
        assert_eq!(lock.inner.write_waiters.len(), 1);
        assert_eq!(lock.inner.read_waiters.len(), 1);

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
        assert_eq!(lock.inner.read_waiters.len(), 2);
        assert_eq!(lock.inner.write_waiters.len(), 0);

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

    /// A waiter that gave up removes its waker, and the lock can be
    /// consumed once it is released.
    #[test]
    fn test_consume_after_cancelled_write_waiter() {
        let lock = LocalRwLock::new(42);
        let mut cx = Context::from_waker(Waker::noop());

        let guard = lock.try_write().unwrap();
        let mut writer = Box::pin(lock.write());
        assert!(writer.as_mut().poll(&mut cx).is_pending());
        drop(writer);
        drop(guard);

        assert_eq!(lock.consume().ok(), Some(42));
    }

    /// Releasing the lock with a writer waiting leaves the readers' queue as
    /// it is; a reader that gave up has removed its waker from it.
    #[test]
    fn test_consume_after_cancelled_read_waiter() {
        let lock = LocalRwLock::new(42);
        let mut cx = Context::from_waker(Waker::noop());

        let guard = lock.try_write().unwrap();
        let mut reader = Box::pin(lock.read());
        let mut writer = Box::pin(lock.write());
        assert!(reader.as_mut().poll(&mut cx).is_pending());
        assert!(writer.as_mut().poll(&mut cx).is_pending());
        drop(reader);
        drop(guard);
        drop(writer);

        assert_eq!(lock.consume().ok(), Some(42));
    }

    #[test]
    fn test_consume_fails_while_locked_or_awaited() {
        let lock = LocalRwLock::new(42);
        let mut cx = Context::from_waker(Waker::noop());

        let guard = lock.try_write().unwrap();
        let lock = lock.consume().expect_err("locked");
        let mut writer = Box::pin(lock.write());
        assert!(writer.as_mut().poll(&mut cx).is_pending());
        drop(guard);
        let lock = lock.consume().expect_err("a writer is waiting");
        drop(writer);

        assert_eq!(lock.consume().ok(), Some(42));
    }

    /// Counts how often it is woken.
    #[derive(Default)]
    struct Wakes(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for Wakes {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// A future waiting for the lock, with a waker that counts.
    struct Waiter<F> {
        future: Pin<Box<F>>,
        wakes: std::sync::Arc<Wakes>,
    }

    impl<F: Future> Waiter<F> {
        fn new(future: F) -> Self {
            Self {
                future: Box::pin(future),
                wakes: Default::default(),
            }
        }

        fn poll(&mut self) -> Poll<F::Output> {
            let waker = Waker::from(self.wakes.clone());
            self.future.as_mut().poll(&mut Context::from_waker(&waker))
        }

        fn wakes(&self) -> usize {
            self.wakes.0.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn expect_pending(&mut self, what: &str) {
            assert!(self.poll().is_pending(), "{what} should wait");
        }

        fn expect_ready(&mut self, what: &str) -> F::Output {
            match self.poll() {
                Poll::Ready(guard) => guard,
                Poll::Pending => panic!("{what} should have the lock"),
            }
        }
    }

    /// A waiter that is woken but finds the lock taken again waits on, and
    /// one that started waiting in the meantime is still woken: three
    /// waiters at once.
    #[test]
    fn test_every_waiter_is_woken_when_a_woken_one_waits_again() {
        let lock = LocalRwLock::new(0);
        let a = lock.try_write().unwrap();
        let mut b = Waiter::new(lock.write());
        b.expect_pending("B");
        drop(a);
        assert_eq!(b.wakes(), 1);

        // The lock is taken again before B gets to it, and C starts waiting.
        let a = lock.try_write().unwrap();
        let mut c = Waiter::new(lock.write());
        c.expect_pending("C");
        b.expect_pending("B, woken");
        drop(a);
        assert_eq!((b.wakes(), c.wakes()), (2, 1));

        let b_guard = b.expect_ready("B");
        c.expect_pending("C");
        drop(b_guard);
        assert_eq!(c.wakes(), 2);
        drop(c.expect_ready("C"));
        drop((b, c));
        assert_eq!(lock.consume().ok(), Some(0));
    }

    /// The same with readers: every reader that waits is woken, also the
    /// ones that started waiting while a woken reader found a writer in its
    /// way.
    #[test]
    fn test_every_reader_is_woken_when_a_woken_one_waits_again() {
        let lock = LocalRwLock::new(0);
        let w = lock.try_write().unwrap();
        let mut r1 = Waiter::new(lock.read());
        let mut r2 = Waiter::new(lock.read());
        r1.expect_pending("R1");
        r2.expect_pending("R2");
        drop(w);
        assert_eq!((r1.wakes(), r2.wakes()), (1, 1));

        let w = lock.try_write().unwrap();
        let mut r3 = Waiter::new(lock.read());
        r3.expect_pending("R3");
        r1.expect_pending("R1, woken");
        drop(w);
        assert_eq!((r1.wakes(), r2.wakes(), r3.wakes()), (2, 1, 1));

        let guards = [
            r1.expect_ready("R1"),
            r2.expect_ready("R2"),
            r3.expect_ready("R3"),
        ];
        drop(guards);
        drop((r1, r2, r3));
        assert_eq!(lock.consume().ok(), Some(0));
    }

    /// Writers go first, readers wait until no writer does, and nobody is
    /// left behind on the way.
    #[test]
    fn test_writers_and_readers_are_all_woken_in_turn() {
        let lock = LocalRwLock::new(0);
        let h = lock.try_write().unwrap();
        let mut w1 = Waiter::new(lock.write());
        let mut r = Waiter::new(lock.read());
        w1.expect_pending("W1");
        r.expect_pending("R");
        drop(h);
        assert_eq!((w1.wakes(), r.wakes()), (1, 0), "only the writer");

        let h = lock.try_write().unwrap();
        let mut w2 = Waiter::new(lock.write());
        w2.expect_pending("W2");
        w1.expect_pending("W1, woken");
        drop(h);
        assert_eq!((w1.wakes(), w2.wakes(), r.wakes()), (2, 1, 0));

        let mut g = w1.expect_ready("W1");
        *g += 1;
        w2.expect_pending("W2");
        r.expect_pending("R");
        drop(g);
        assert_eq!((w2.wakes(), r.wakes()), (2, 0));

        let mut g = w2.expect_ready("W2");
        *g += 1;
        r.expect_pending("R");
        drop(g);
        assert_eq!(r.wakes(), 1);
        assert_eq!(*r.expect_ready("R"), 2);
        drop((w1, w2, r));
        assert_eq!(lock.consume().ok(), Some(2));
    }

    /// A waiter that gives up after it was woken removes nothing but its own
    /// waker, also when another waiter started waiting in the meantime.
    #[test]
    fn test_a_woken_waiter_that_gives_up_leaves_the_others_alone() {
        let lock = LocalRwLock::new(0);
        let a = lock.try_write().unwrap();
        let mut b = Waiter::new(lock.write());
        b.expect_pending("B");
        drop(a);

        let a = lock.try_write().unwrap();
        let mut c = Waiter::new(lock.write());
        c.expect_pending("C");
        let mut d = Waiter::new(lock.write());
        d.expect_pending("D");
        drop(b);
        drop(a);
        assert_eq!((c.wakes(), d.wakes()), (1, 1));

        let c_guard = c.expect_ready("C");
        d.expect_pending("D, woken");
        drop(c_guard);
        assert_eq!(d.wakes(), 2);
        drop(d.expect_ready("D"));
        drop((c, d));
        assert_eq!(lock.consume().ok(), Some(0));
    }
}
