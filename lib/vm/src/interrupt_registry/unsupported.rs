use wasmer_types::StoreId;

use super::*;

/// Selects the process-wide signal used to interrupt running WASM code.
///
/// On unsupported platforms this is a no-op.
pub fn set_interrupt_signal(_signal: InterruptSignal) -> Result<(), SetInterruptSignalError> {
    Ok(())
}

/// Returns the signal currently selected for interrupting WASM code.
///
/// On unsupported platforms no signal is ever used, so this always reports
/// the default.
pub fn interrupt_signal() -> InterruptSignal {
    DEFAULT_INTERRUPT_SIGNAL
}

/// Install interrupt state for the given store.
///
/// On unsupported platforms this is a no-op.
pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    Ok(InterruptInstallGuard { store_id })
}

pub(super) fn uninstall(_store_id: StoreId) {}

/// Interrupt the given store.
///
/// On unsupported platforms this is a no-op.
pub fn interrupt(_store_id: StoreId) -> Result<(), InterruptError> {
    Ok(())
}

/// Returns whether the given store has been interrupted.
///
/// On unsupported platforms interrupts are not tracked.
pub fn is_interrupted(_store_id: StoreId) -> bool {
    false
}
