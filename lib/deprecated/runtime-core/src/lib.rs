#![allow(deprecated)]

pub(crate) mod new {
    pub use wasm_common;
    pub use wasmer;
    pub use wasmer_compiler;
    pub use wasmer_engine;
    pub use wasmer_runtime;
}

pub mod backend;
pub mod cache;
pub mod error;
mod functional_api;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod table;
pub mod types;
pub mod units;

pub use crate::cache::Artifact;
pub use crate::import::IsExport;
pub use crate::instance::Instance;
pub use crate::module::Module;
pub use crate::new::wasmer_compiler::wasmparser;
pub use crate::units::{Bytes, Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
pub use functional_api::{compile_with, compile_with_config, validate};

pub mod prelude {
    pub use crate::import::{ImportObject, Namespace};
}
