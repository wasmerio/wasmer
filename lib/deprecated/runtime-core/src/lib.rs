mod instance;
pub mod types;

mod new {
    pub(crate) use wasm_common;
    pub(crate) use wasmer;
}

pub use crate::instance::Instance;
