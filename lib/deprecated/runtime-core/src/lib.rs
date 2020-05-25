#![allow(deprecated)]

pub(crate) mod new {
    pub use wasm_common;
    pub use wasmer;
    pub use wasmer_runtime;
}

mod functional_api;
pub mod instance;
pub mod import {
    pub use crate::new::wasmer::{imports, ImportObject, ImportObjectIterator};
}
pub mod module;
pub mod types;
pub mod units;

pub use crate::instance::Instance;
pub use crate::module::Module;
pub use crate::units::{Bytes, Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
pub use functional_api::{compile_with, compile_with_config, validate};
