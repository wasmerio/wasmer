//! Wasmer API
#![deny(intra_doc_link_resolution_failure)]

mod exports;
mod externals;
mod import_object;
mod instance;
mod memory_view;
mod module;
mod ptr;
mod store;
mod types;

pub use crate::exports::{ExportError, Exportable, Exports};
pub use crate::externals::{Extern, Func, Global, Memory, Table};
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::Instance;
pub use crate::memory_view::MemoryView;
pub use crate::module::Module;
pub use crate::ptr::{Array, Item, WasmPtr};
pub use crate::store::{Engine, Store, StoreObject};
pub use crate::types::{
    AnyRef, ExportType, ExternType, FuncType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};

pub use wasm_common::{ValueType, WasmExternType, WasmTypeList};
pub use wasmer_compiler::CompilerConfig;
pub use wasmer_jit::{
    DeserializeError, InstantiationError, LinkError, RuntimeError, SerializeError,
};

#[cfg(feature = "compiler-singlepass")]
pub use wasmer_compiler_singlepass::SinglepassConfig;

#[cfg(feature = "compiler-cranelift")]
pub use wasmer_compiler_cranelift::CraneliftConfig;

#[cfg(feature = "compiler-llvm")]
pub use wasmer_compiler_llvm::LLVMConfig;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
