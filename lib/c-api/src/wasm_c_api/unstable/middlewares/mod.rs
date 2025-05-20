//! Unstable non-standard Wasmer-specific types to manipulate module
//! middlewares.

pub mod metering;

use std::sync::Arc;
use wasmer_api::sys::ModuleMiddleware;

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
