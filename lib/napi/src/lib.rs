mod ctx;
mod env;
mod guest;
mod snapi;
#[cfg(feature = "wasix")]
mod wasix;

use std::sync::LazyLock;

pub use ctx::{NapiCtx, NapiCtxBuilder, NapiLimits, NapiRuntimeHooks, NapiSession};
pub(crate) use env::{GuestBackingStoreMapping, HostBufferCopy, RuntimeEnv};

pub fn module_needs_napi(module: &wasmer::Module) -> bool {
    NapiCtx::module_needs_napi(module)
}

const NAPI_CURRENT_VERSION: u64 = 10;
const NAPI_EXTENSION_WASMER_CURRENT_VERSION: u64 = 0;

pub static NAPI_INTERFACE_NAME: LazyLock<String> =
    LazyLock::new(|| format!("napi_v{NAPI_CURRENT_VERSION}"));
pub static NAPI_EXTENSION_WASMER_INTERFACE_NAME: LazyLock<String> =
    LazyLock::new(|| format!("napi_extension_wasmer_v{NAPI_EXTENSION_WASMER_CURRENT_VERSION}"));
