pub mod metering;

use super::super::engine::wasm_config_t;
use std::sync::Arc;
use wasmer_compiler::ModuleMiddleware;

#[cfg(all(feature = "middlewares", not(feature = "compiler")))]
compile_error!("The `middlewares` feature requires the `compiler` feature to be turned on");

/// Opaque representing a middleware.
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasmer_middleware_t {
    pub(in crate::wasm_c_api) inner: Arc<dyn ModuleMiddleware>,
}

#[no_mangle]
pub extern "C" fn wasm_config_push_middleware(
    config: &mut wasm_config_t,
    middleware: Box<wasmer_middleware_t>,
) {
    config.middlewares.push(*middleware);
}
