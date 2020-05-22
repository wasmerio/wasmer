#![allow(deprecated)]

mod functional_api;
pub mod instance;
pub mod module;
pub mod types;
pub mod units;

pub(crate) mod new {
    pub use wasm_common;
    pub use wasmer;
    pub use wasmer_runtime;
}

pub use crate::instance::Instance;
pub use crate::module::Module;
pub use functional_api::{compile_with, compile_with_config, validate};

pub mod import {
    pub use crate::new::wasmer::{imports, ImportObject, ImportObjectIterator};
}
