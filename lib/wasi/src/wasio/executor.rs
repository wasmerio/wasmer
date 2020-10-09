//! Asynchronous executor for WASIO operations.

use super::types::*;
use crate::syscalls::types::*;
use std::fmt::{self, Debug};
use std::sync::Arc;
use std::cell::RefCell;
use std::any::Any;

/// The `Executor` trait.
pub trait Executor: Send {
    /// Enqueues an asynchronous oneshot operation.
    ///
    /// Returns a `CancellationToken` if the operation is successfully enqueued, and an error code otherwise.
    fn enqueue_oneshot(
        &self,
        _: AsyncOneshotOperation,
        _: UserContext,
    ) -> Result<CancellationToken, __wasi_errno_t> {
        Err(__WASI_ENOTSUP)
    }

    /// Enqueues an asynchronous stream operation.
    ///
    /// Returns a `CancellationToken` if the operation is successfully enqueued, and an error code otherwise.
    fn enqueue_stream(
        &self,
        _: AsyncStreamOperation,
        _: UserContext,
    ) -> Result<CancellationToken, __wasi_errno_t> {
        Err(__WASI_ENOTSUP)
    }

    /// Performs a synchronous operation.
    fn perform(&self, _: SyncOperation) -> Result<(), __wasi_errno_t> {
        Err(__WASI_ENOTSUP)
    }

    /// Blocks the current thread and wait for the next completed operation.
    fn wait(&self) -> Result<(__wasi_errno_t, UserContext), __wasi_errno_t> {
        Err(__WASI_ENOTSUP)
    }

    fn as_any(&self) -> &dyn Any;
}

impl Debug for dyn Executor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<wasio executor>")
    }
}

/// The current executor on this thread.
thread_local! {
    static CURRENT: RefCell<Option<Arc<dyn Executor>>> = RefCell::new(None);
}

pub fn make_current<F: FnOnce() -> R, R>(e: Arc<dyn Executor>, f: F) -> R {
    CURRENT.with(|x| {
        let prev = x.borrow_mut().replace(e);
        let ret = f();
        *x.borrow_mut() = prev;
        ret
    })
}

pub fn with_current<T: 'static, F: FnOnce(Option<&T>) -> R, R>(f: F) -> R {
    CURRENT.with(|x| {
        let inner = x.borrow().clone();
        let downcasted = match inner {
            Some(ref x) => match x.as_any().downcast_ref::<T>() {
                Some(x) => Some(x),
                None => None
            }
            None => None
        };
        f(downcasted)
    })
}