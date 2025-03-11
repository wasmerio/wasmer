use crate::{
    error::update_last_error,
    wasm_c_api::engine::{wasm_config_t, wasm_engine_t, wasmer_backend_t},
};

use super::wasmer_backend_config_kind_t;

/// Configuration specific for the `wasmi` engine.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
///
/// cbindgen:ignore
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct wasmer_wasmi_engine_config_t;

/// Create a new  [`wasm_engine_t`] backed by a `wasmi` engine.
pub(crate) fn wasm_wasmi_engine_new_with_config(
    config: wasm_config_t,
) -> Option<Box<wasm_engine_t>> {
    if !matches!(config.backend, wasmer_backend_t::WASMI) || !config.backend_config.inner.is_wasmi()
    {
        update_last_error("Cannot create a new `wasmi` engine with a non-wasmi-specific config!");
        return None;
    }

    Some(Box::new(wasm_engine_t {
        inner: wasmer_api::wasmi::Wasmi::default().into(),
    }))
}

impl wasmer_backend_config_kind_t {
    /// Returns `true` if the wasmer_engine_config_t is [`WASMI`].
    ///
    /// [`WASMI`]: wasmer_engine_config_t::WASMI
    #[must_use]
    pub(super) fn is_wasmi(&self) -> bool {
        matches!(self, Self::Wasmi(..))
    }
}
