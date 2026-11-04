//! Locks that a host panic does not wedge, as the kernel's: a panic in one syscall poisons the
//! locks it held, and taking over the guard keeps every other user of the namespace working.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// `Mutex::lock` that takes over the guard of a lock a panicking thread held.
pub(crate) trait MutexExt<T: ?Sized> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
