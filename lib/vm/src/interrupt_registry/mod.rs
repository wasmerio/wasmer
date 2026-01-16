//! Implements the necessary infrastructure for interrupting running WASM code
//! via OS signals.
//!
//! This module is meant to be used from within the wasmer crate. Embedders
//! should not call any of the functions here; instead, they should go
//! through [`wasmer::Store::get_interrupter`].

// TODO: Windows support

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
