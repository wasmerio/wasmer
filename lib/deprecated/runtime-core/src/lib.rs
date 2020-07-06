#![allow(deprecated)]

pub(crate) mod new {
    pub use wasm_common;
    pub use wasmer;
    pub use wasmer_cache;
    pub use wasmer_compiler;
    pub use wasmer_engine;
    pub use wasmer_runtime;
}

pub mod backend;
pub mod cache;
pub mod error;
pub mod export;
mod functional_api;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod structures;
pub mod table;
pub mod typed_func;
pub mod types;
pub mod units;
pub mod vm;

pub use crate::cache::{Artifact, WasmHash};
pub use crate::import::IsExport;
pub use crate::instance::{DynFunc, Exports, Instance};
pub use crate::module::Module;
pub use crate::new::wasmer_compiler::wasmparser;
pub use crate::typed_func::{DynamicFunc, Func};
pub use crate::units::{Bytes, Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
pub use functional_api::{
    compile, compile_with, compile_with_config, load_cache_with, validate, wat2wasm,
};

pub mod prelude {
    pub use crate::import::{namespace, ImportObject, Namespace};
    pub use crate::typed_func::{DynamicFunc, Func};
    pub use crate::types::{FuncIndex, GlobalIndex, MemoryIndex, TableIndex, Type, Value};
}

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

lazy_static::lazy_static! {
    static ref GLOBAL_STORE: new::wasmer::Store = Default::default();
}

/// Useful if one needs to migrate to the new Wasmer's API gently.
pub fn get_global_store() -> &'static new::wasmer::Store {
    &*GLOBAL_STORE
}
