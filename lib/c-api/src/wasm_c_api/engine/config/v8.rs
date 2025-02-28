use crate::{
    error::update_last_error,
    wasm_c_api::engine::{wasm_config_t, wasm_engine_t, wasmer_engine_t},
};

use super::wasmer_engine_config_t;

/// Configuration specific for the `v8` engine.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
///
/// cbindgen:ignore
#[repr(C)]
#[derive(Debug, Default)]
pub struct wasmer_v8_engine_config_t;

/// Create a new  [`wasm_engine_t`] backed by a `v8` engine.
pub fn wasm_v8_engine_new_with_config(config: wasm_config_t) -> Option<Box<wasm_engine_t>> {
    if !matches!(config.engine, wasmer_engine_t::V8) || !config.engine_config.is_v8() {
        update_last_error("Cannot create a new `v8` engine with a non-v8-specific config!");
        return None;
    }

    Some(Box::new(wasm_engine_t {
        inner: wasmer_api::v8::V8::default().into(),
    }))
}

impl wasmer_engine_config_t {
    /// Returns `true` if the wasmer_engine_config_t is [`V8`].
    ///
    /// [`V8`]: wasmer_engine_config_t::V8
    #[must_use]
    pub(super) fn is_v8(&self) -> bool {
        matches!(self, Self::V8(..))
    }
}
