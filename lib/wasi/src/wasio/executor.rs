//! Asynchronous executor for WASIO operations.

use super::types::*;
use crate::syscalls::types::*;
use std::fmt::{self, Debug};

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
}

impl Debug for dyn Executor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<wasio executor>")
    }
}
