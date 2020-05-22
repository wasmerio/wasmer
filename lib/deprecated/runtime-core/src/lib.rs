mod instance;
pub mod types;

pub(crate) mod new {
    pub use wasm_common;
    pub use wasmer;
}

pub use crate::instance::Instance;
