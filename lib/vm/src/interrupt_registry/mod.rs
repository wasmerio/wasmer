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

/// The OS signal used to interrupt running WASM code.
///
/// Since the signal handler is installed process-wide, embedders that use
/// one of these signals for their own purposes can pick the other one
/// through [`set_interrupt_signal`]. The set of choices is intentionally
/// constrained: any other signal either has a well-defined meaning that
/// must not be hijacked, or can't be reliably delivered to a specific
/// thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptSignal {
    /// `SIGUSR1`. This is the default.
    Sigusr1,
    /// `SIGUSR2`.
    Sigusr2,
}

/// The signal Wasmer uses to interrupt running WASM code unless the
/// embedder selects a different one.
pub const DEFAULT_INTERRUPT_SIGNAL: InterruptSignal = InterruptSignal::Sigusr1;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum SetInterruptSignalError {
    #[error(
        "The interrupt signal was already set to {current:?} and can't be changed to {requested:?}"
    )]
    AlreadySet {
        current: InterruptSignal,
        requested: InterruptSignal,
    },
    #[error(
        "The interrupt signal handler was already installed for {current:?}, so it can't be \
         changed to {requested:?}; the interrupt signal must be selected before creating any \
         Wasmer engine or store"
    )]
    HandlerAlreadyInstalled {
        current: InterruptSignal,
        requested: InterruptSignal,
    },
}

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
