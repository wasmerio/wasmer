mod ctx;
mod env;
mod guest;
mod snapi;
#[cfg(feature = "wasix")]
mod wasix;


pub(crate) use env::RuntimeEnv;
pub use ctx::{NapiCtx, NapiCtxBuilder, NapiLimits, NapiRuntimeHooks, NapiSession};

pub fn module_needs_napi(module: &wasmer::Module) -> bool {
    NapiCtx::module_needs_napi(module)
}
