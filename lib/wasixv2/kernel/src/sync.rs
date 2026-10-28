//! Locks that a host panic does not wedge. The execution layer catches a panic in a syscall and
//! ends only the process that made it, but a lock the panicking thread held is poisoned, and
//! every other task that took it with `lock().unwrap()` would panic too and take its own process
//! down: one bad syscall would end every process that touches the same object. Taking over the
//! guard instead leaves whatever the panicking syscall changed as it was, as Linux carries on
//! after an oops in a task.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// `Mutex::lock` that takes over the guard of a lock a panicking thread held.
pub(crate) trait MutexExt<T: ?Sized> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `RwLock::read` and `RwLock::write` that take over the guard of a lock a panicking thread
/// held.
pub(crate) trait RwLockExt<T: ?Sized> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T>;
    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLockExt<T> for RwLock<T> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    #[test]
    fn a_mutex_a_panicking_thread_held_still_locks() {
        let mutex = Arc::new(Mutex::new(1));
        let held = mutex.clone();
        let result = thread::spawn(move || {
            let mut guard = held.lock_or_recover();
            *guard = 2;
            panic!("a host panic in a syscall, with the lock held");
        })
        .join();
        assert!(result.is_err());
        assert!(mutex.is_poisoned());
        assert_eq!(*mutex.lock_or_recover(), 2);
    }

    #[test]
    fn an_rwlock_a_panicking_thread_held_still_reads_and_writes() {
        let lock = Arc::new(RwLock::new(1));
        let held = lock.clone();
        let result = thread::spawn(move || {
            let mut guard = held.write_or_recover();
            *guard = 2;
            panic!("a host panic in a syscall, with the lock held");
        })
        .join();
        assert!(result.is_err());
        assert!(lock.is_poisoned());
        assert_eq!(*lock.read_or_recover(), 2);
        *lock.write_or_recover() = 3;
        assert_eq!(*lock.read_or_recover(), 3);
    }
}
