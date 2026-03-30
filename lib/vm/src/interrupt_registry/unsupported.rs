use wasmer_types::StoreId;

use super::*;

/// Install interrupt state for the given store.
///
/// On unsupported platforms this is a no-op.
pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    Ok(InterruptInstallGuard { store_id })
}

pub(super) fn uninstall(store_id: StoreId) {}

/// Interrupt the given store.
///
/// On unsupported platforms this is a no-op.
pub fn interrupt(store_id: StoreId) -> Result<(), InterruptError> {
    Ok(())
}
