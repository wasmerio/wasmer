use crate::{
    error::update_last_error,
    wasm_c_api::engine::{wasm_config_t, wasm_engine_t, wasmer_backend_t},
};

use super::wasmer_backend_config_kind_t;

/// Configuration specific for the `wamr` engine.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
///
/// cbindgen:ignore
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct wasmer_wamr_engine_config_t;

/// Create a new  [`wasm_engine_t`] backed by a `wamr` engine.
pub(crate) fn wasm_wamr_engine_new_with_config(
    config: wasm_config_t,
) -> Option<Box<wasm_engine_t>> {
    if !matches!(config.backend, wasmer_backend_t::WAMR) || !config.backend_config.inner.is_wamr() {
        update_last_error("Cannot create a new `wamr` engine with a non-wamr-specific config!");
        return None;
    }

    Some(Box::new(wasm_engine_t {
        inner: wasmer_api::wamr::Wamr::default().into(),
    }))
}

impl wasmer_backend_config_kind_t {
    /// Returns `true` if the wasmer_engine_config_t is [`WAMR`].
    ///
    /// [`WAMR`]: wasmer_engine_config_t::WAMR
    #[must_use]
    pub(super) fn is_wamr(&self) -> bool {
        matches!(self, Self::Wamr(..))
    }
}
