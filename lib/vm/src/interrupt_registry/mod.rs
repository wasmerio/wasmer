//! Implements the necessary infrastructure for interrupting running WASM code
//! via OS signals.
//!
//! This module is meant to be used from within the wasmer crate. Embedders
//! should not call any of the functions here; instead, they should go
//! through [`wasmer::Store::get_interrupter`].

// TODO: Windows support

use std::sync::Arc;

use thiserror::Error;
use wasmer_types::StoreId;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

// The unsupported module implements no-op functions instead of panicking;
// this lets us avoid a bunch of #[cfg]'s everywhere in the runtime code.
#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub use unsupported::*;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum InstallError {
    #[error("This store was already interrupted and can't be entered again")]
    AlreadyInterrupted,
}

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum InterruptError {
    #[error("Store not running")]
    StoreNotRunning,
    #[error("Another interrupt is already in progress on the target thread")]
    OtherInterruptInProgress,
    #[error("Failed to send interrupt signal due to OS error: {0}")]
    FailedToSendSignal(&'static str),
}

/// Wakes native work that has temporarily moved off the Wasm stack.
///
/// Store interrupts normally redirect the Wasm coroutine from the signal
/// handler. Native waits cannot be redirected safely because doing so would
/// skip Rust destructors and leave any held locks permanently acquired.
pub(crate) trait InterruptWaitWaker: Send + Sync {
    fn wake(&self);
}

/// Removes a native interrupt waiter when the wait completes.
pub(crate) struct InterruptWaitGuard {
    store_id: StoreId,
    waker: Arc<dyn InterruptWaitWaker>,
}

impl Drop for InterruptWaitGuard {
    fn drop(&mut self) {
        unregister_wait(self.store_id, &self.waker);
    }
}

/// Uninstalls interrupt state when dropped
pub struct InterruptInstallGuard {
    store_id: StoreId,
}

impl Drop for InterruptInstallGuard {
    fn drop(&mut self) {
        let store_id = self.store_id;
        uninstall(store_id);
    }
}
