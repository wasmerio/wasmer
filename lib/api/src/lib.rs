//! Wasmer API
#![deny(intra_doc_link_resolution_failure)]

mod exports;
mod externals;
mod import_object;
mod instance;
mod module;
mod store;
mod types;

pub use crate::exports::{ExportError, Exportable, Exports};
pub use crate::externals::{Extern, Func, Global, Memory, Table};
#[macro_use]
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::Instance;
pub use crate::module::Module;
pub use crate::store::{Engine, Store, StoreObject};
pub use crate::types::{
    AnyRef, ExportType, ExternType, FuncType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};
pub use wasmer_compiler_cranelift::CraneliftConfig as Config;
pub use wasmer_jit::{
    CompilerConfig, DeserializeError, InstantiationError, LinkError, RuntimeError, SerializeError,
};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
