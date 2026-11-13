use wasmer_types::StoreId;

use super::*;

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

/// On unsupported platforms no store is ever registered as active.
pub(crate) struct ActiveStores;

impl ActiveStores {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn is_empty(&self) -> bool {
        true
    }

    pub(crate) fn is_interrupted(&self, _store_id: StoreId) -> bool {
        false
    }

    pub(crate) fn swap(&mut self) {}
}
