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
mod tunables;
mod types;

pub use crate::exports::{ExportError, Exportable, Exports};
pub use crate::externals::{Extern, Function, Global, Memory, Table};
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::Instance;
pub use crate::memory_view::MemoryView;
pub use crate::module::Module;
pub use crate::ptr::{Array, Item, WasmPtr};
pub use crate::store::{Store, StoreObject};
pub use crate::tunables::{MemoryError, Tunables};
pub use crate::types::{
    AnyRef, ExportType, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};

pub use wasm_common::{Bytes, Pages, ValueType, WasmExternType, WasmTypeList};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::CompilerConfig;
pub use wasmer_compiler::{Features, Target};
pub use wasmer_engine::{
    DeserializeError, Engine, InstantiationError, LinkError, RuntimeError, SerializeError,
};

// The compilers are mutually exclusive
#[cfg(any(
    all(feature = "llvm", any(feature = "cranelift", feature = "singlepass")),
    all(feature = "cranelift", feature = "singlepass")
))]
compile_error!(
    r#"The `singlepass`, `cranelift` and `llvm` features are mutually exclusive.
If you wish to use more than one compiler, you can simply import it from it's own crate. Eg.:

```
use wasmer::{Store, Engine};
use wasmer_compiler_singlepass::SinglepassConfig;

let engine = Engine::new(SinglepassConfig::default());
let store = Store::new_config(&engine);
```"#
);

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::SinglepassConfig;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::CraneliftConfig;

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::LLVMConfig;

#[cfg(feature = "jit")]
pub use wasmer_engine_jit::JITEngine;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
