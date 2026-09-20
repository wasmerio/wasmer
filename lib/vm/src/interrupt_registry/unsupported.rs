use wasmer_types::StoreId;

use std::sync::Arc;

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

pub(crate) fn register_wait(
    store_id: StoreId,
    waker: Arc<dyn InterruptWaitWaker>,
) -> Option<InterruptWaitGuard> {
    Some(InterruptWaitGuard { store_id, waker })
}

pub(super) fn unregister_wait(_store_id: StoreId, _waker: &Arc<dyn InterruptWaitWaker>) {}
