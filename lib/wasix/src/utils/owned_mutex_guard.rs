/// Extends the standard library RwLock to include an owned version of the read
/// and write locking functions.
///
/// This implementation contains unsafe code and has two levels of protection
/// that prevent the lock from being released after the memory is freed.
///
/// 1. The internals use a Option which is cleared before the Drop completes
/// 2. The Arc reference is placed as the last field which should be dropped last
///    (https://doc.rust-lang.org/reference/destructors.html#:~:text=The%20fields%20of%20a%20struct,first%20element%20to%20the%20last.)
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, LockResult, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks this rwlock with shared read access, blocking the current thread
/// until it can be acquired.
///
/// The calling thread will be blocked until there are no more writers which
/// hold the lock. There may be other readers currently inside the lock when
/// this method returns. This method does not provide any guarantees with
/// respect to the ordering of whether contentious readers or writers will
/// acquire the lock first.
///
/// Returns an RAII guard which will release this thread's shared access
/// once it is dropped.
///
/// # Errors
///
/// This function will return an error if the RwLock is poisoned. An RwLock
/// is poisoned whenever a writer panics while holding an exclusive lock.
/// The failure will occur immediately after the lock has been acquired.
///
/// # Panics
///
/// This function might panic when called if the lock is already held by the current thread.
///
// # Examples
//
// ```
// use std::sync::{Arc, RwLock};
// use std::thread;
// use crate::utils::owned_mutex_guard::read_owned;
//
// let lock = Arc::new(RwLock::new(1));
// let c_lock = Arc::clone(&lock);
//
// let n = read_owned(&lock).unwrap();
// assert_eq!(*n, 1);
//
// thread::spawn(move || {
//     let r = read_owned(&c_lock);
//     assert!(r.is_ok());
// }).join().unwrap();
// ```
pub(crate) fn read_owned<T>(lock: &Arc<RwLock<T>>) -> LockResult<OwnedRwLockReadGuard<T>> {
    OwnedRwLockReadGuard::new(lock)
}

/// Locks this rwlock with exclusive write access, blocking the current
/// thread until it can be acquired.
///
/// This function will not return while other writers or other readers
/// currently have access to the lock.
///
/// Returns an RAII guard which will drop the write access of this rwlock
/// when dropped.
///
/// # Errors
///
/// This function will return an error if the RwLock is poisoned. An RwLock
/// is poisoned whenever a writer panics while holding an exclusive lock.
/// An error will be returned when the lock is acquired.
///
/// # Panics
///
/// This function might panic when called if the lock is already held by the current thread.
///
// # Examples
//
// ```
// use std::sync::RwLock;
// use crate::utils::owned_mutex_guard::write_owned;
//
// let lock = RwLock::new(1);
//
// let mut n = write_owned(&lock).unwrap();
// *n = 2;
// ```
pub(crate) fn write_owned<T>(lock: &Arc<RwLock<T>>) -> LockResult<OwnedRwLockWriteGuard<T>> {
    OwnedRwLockWriteGuard::new(lock)
}

pub(crate) struct OwnedRwLockReadGuard<T: 'static> {
    // This option is guaranteed to be `.is_some()` while in scope and cleared during the `Drop`
    guard: Option<RwLockReadGuard<'static, T>>,
    // as a precaution we keep the reference as the last field so that it is destructed after the guard
    // (https://doc.rust-lang.org/reference/destructors.html#:~:text=The%20fields%20of%20a%20struct,first%20element%20to%20the%20last.)
    #[allow(unused)]
    ownership: Arc<RwLock<T>>,
}

unsafe impl<T> Send for OwnedRwLockReadGuard<T> where T: Send {}

impl<T> Drop for OwnedRwLockReadGuard<T>
where
    T: Sized,
{
    fn drop(&mut self) {
        // we must close the lock before we release the arc reference
        self.guard.take();
    }
}

impl<T> std::fmt::Debug for OwnedRwLockReadGuard<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(guard) = self.guard.as_ref() {
            write!(f, "{guard:?}")
        } else {
            write!(f, "none")
        }
    }
}

impl<T> std::fmt::Display for OwnedRwLockReadGuard<T>
where
    T: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(guard) = self.guard.as_ref() {
            write!(f, "{guard}")
        } else {
            write!(f, "none")
        }
    }
}

impl<T> OwnedRwLockReadGuard<T> {
    fn new(lock: &Arc<RwLock<T>>) -> LockResult<Self> {
        let conv = |guard: RwLockReadGuard<'_, T>| {
            let guard: RwLockReadGuard<'static, T> = unsafe { std::mem::transmute(guard) };
            Self {
                ownership: lock.clone(),
                guard: Some(guard),
            }
        };
        let guard = lock.read().map_err(|err| {
            let guard = err.into_inner();
            PoisonError::new(conv(guard))
        })?;
        Ok(conv(guard))
    }

    /// Converts this guard into an owned reference of the underlying lockable object
    #[allow(dead_code)]
    pub fn into_inner(self) -> Arc<RwLock<T>> {
        self.ownership.clone()
    }
}

impl<T> Deref for OwnedRwLockReadGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}

pub(crate) struct OwnedRwLockWriteGuard<T: 'static> {
    // This option is guaranteed to be `.is_some()` while in scope and cleared during the `Drop`
    guard: Option<RwLockWriteGuard<'static, T>>,
    // as a precaution we keep the reference as the last field so that it is destructed after the guard
    // (https://doc.rust-lang.org/reference/destructors.html#:~:text=The%20fields%20of%20a%20struct,first%20element%20to%20the%20last.)
    #[allow(unused)]
    ownership: Arc<RwLock<T>>,
}

unsafe impl<T> Send for OwnedRwLockWriteGuard<T> where T: Send {}

impl<T> Drop for OwnedRwLockWriteGuard<T>
where
    T: Sized,
{
    fn drop(&mut self) {
        // we must close the lock before we release the arc reference
        self.guard.take();
    }
}

impl<T> std::fmt::Debug for OwnedRwLockWriteGuard<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(guard) = self.guard.as_ref() {
            write!(f, "{guard:?}")
        } else {
            write!(f, "none")
        }
    }
}

impl<T> std::fmt::Display for OwnedRwLockWriteGuard<T>
where
    T: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(guard) = self.guard.as_ref() {
            write!(f, "{guard}")
        } else {
            write!(f, "none")
        }
    }
}

impl<T> OwnedRwLockWriteGuard<T> {
    fn new(lock: &Arc<RwLock<T>>) -> LockResult<Self> {
        let conv = |guard: RwLockWriteGuard<'_, T>| {
            let guard: RwLockWriteGuard<'static, T> = unsafe { std::mem::transmute(guard) };
            Self {
                ownership: lock.clone(),
                guard: Some(guard),
            }
        };
        let guard = lock.write().map_err(|err| {
            let guard = err.into_inner();
            PoisonError::new(conv(guard))
        })?;
        Ok(conv(guard))
    }

    /// Converts this guard into an owned reference of the underlying lockable object
    #[allow(dead_code)]
    pub fn into_inner(self) -> Arc<RwLock<T>> {
        self.ownership.clone()
    }
}

impl<T> Deref for OwnedRwLockWriteGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}

impl<T> DerefMut for OwnedRwLockWriteGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().unwrap()
    }
}
