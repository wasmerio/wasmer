use wasmer_types::StoreId;

use super::*;

pub fn install(store_id: StoreId) -> Result<InterruptInstallGuard, InstallError> {
    Ok(InterruptInstallGuard { store_id })
}

pub(super) fn uninstall(store_id: StoreId) {}

pub fn interrupt(store_id: StoreId) -> Result<(), InterruptError> {
    Ok(())
}
