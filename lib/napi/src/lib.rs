mod ctx;
mod env;
mod guest;
mod snapi;
#[cfg(feature = "wasix")]
mod wasix;

pub use ctx::{NapiCtx, NapiCtxBuilder, NapiLimits, NapiRuntimeHooks, NapiSession};
pub(crate) use env::{GuestBackingStoreMapping, HostBufferCopy, RuntimeEnv};

pub fn module_needs_napi(module: &wasmer::Module) -> bool {
    NapiCtx::module_needs_napi(module)
}

pub static NAPI_INTERFACE_NAME: &str = "napi_v10";
pub static NAPI_EXTENSION_WASMER_INTERFACE_NAME: &str = "napi_extension_wasmer_v0";
