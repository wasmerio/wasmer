use crate::{
    error::update_last_error,
    wasm_c_api::engine::{wasm_config_t, wasm_engine_t, wasmer_engine_t},
};

use super::wasmer_engine_config_t;

/// Configuration specific for the `jsc` engine.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
///
/// cbindgen:ignore
#[repr(C)]
#[derive(Debug, Default)]
pub struct wasmer_jsc_engine_config_t;

/// Create a new  [`wasm_engine_t`] backed by a `jsc` engine.
pub fn wasm_jsc_engine_new_with_config(config: wasm_config_t) -> Option<Box<wasm_engine_t>> {
    if !matches!(config.engine, wasmer_engine_t::JSC) || !config.engine_config.is_jsc() {
        update_last_error("Cannot create a new `sys` engine with a non-sys-specific config!");
        return None;
    }

    Some(Box::new(wasm_engine_t {
        inner: wasmer_api::jsc::Engine::default().into(),
    }))
}

impl wasmer_engine_config_t {
    /// Returns `true` if the wasmer_engine_config_t is [`Jsc`].
    ///
    /// [`Jsc`]: wasmer_engine_config_t::Jsc
    #[must_use]
    pub(super) fn is_jsc(&self) -> bool {
        matches!(self, Self::Jsc(..))
    }
}
