//! Unstable non-standard Wasmer-specific types to manipulate module
//! middlewares.

pub mod metering;

use super::super::engine::wasm_config_t;
use std::sync::Arc;
use wasmer_api::ModuleMiddleware;

#[cfg(all(feature = "middlewares", not(feature = "compiler")))]
compile_error!("The `middlewares` feature requires the `compiler` feature to be turned on");

/// Opaque representing any kind of middleware.
///
/// Used by `wasm_config_push_middleware`. A specific middleware is
/// transformed into this type to get a generic middleware. See for
/// example
/// [`wasmer_metering_as_middleware`][metering::wasmer_metering_as_middleware].
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasmer_middleware_t {
    pub(in crate::wasm_c_api) inner: Arc<dyn ModuleMiddleware>,
}

/// Updates the configuration to add a module middleware.
///
/// This function takes ownership of `middleware`.
///
/// This is a Wasmer-specific function.
///
/// # Example
///
/// See the documentation of the [`metering`] module.
#[no_mangle]
pub extern "C" fn wasm_config_push_middleware(
    config: &mut wasm_config_t,
    middleware: Box<wasmer_middleware_t>,
) {
    config.middlewares.push(*middleware);
}
