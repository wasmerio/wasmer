//! Wasmer API
#![deny(
    missing_docs,
    trivial_numeric_casts,
    unused_extern_crates,
    intra_doc_link_resolution_failure
)]
#![warn(unused_import_braces)]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, vtable_address_comparisons)
)]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::option_map_unwrap_or,
        clippy::option_map_unwrap_or_else,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

mod exports;
mod externals;
mod import_object;
mod instance;
mod module;
mod native;
mod ptr;
mod store;
mod tunables;
mod types;
mod utils;

pub mod internals {
    //! We use the internals module for exporting types that are only
    //! intended to use in internal crates such as the compatibility crate
    //! `wasmer-vm`. Please don't use any of this types directly, as
    //! they might change frequently or be removed in the future.

    pub use crate::externals::{WithEnv, WithoutEnv};
}

pub use crate::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, Table, WasmTypeList,
};
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::Instance;
pub use crate::module::Module;
pub use crate::native::NativeFunc;
pub use crate::ptr::{Array, Item, WasmPtr};
pub use crate::store::{Store, StoreObject};
pub use crate::tunables::Tunables;
pub use crate::types::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Val, ValType,
};
pub use crate::types::{Val as Value, ValType as Type};
pub use crate::utils::is_wasm;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, FunctionMiddlewareGenerator,
    MiddlewareReaderState,
};
pub use wasmer_compiler::{CpuFeature, Features, Target};
pub use wasmer_engine::{
    ChainableNamedResolver, DeserializeError, Engine, InstantiationError, LinkError, NamedResolver,
    NamedResolverChain, Resolver, RuntimeError, SerializeError,
};
pub use wasmer_types::{
    Atomically, Bytes, GlobalInit, LocalFunctionIndex, MemoryView, Pages, ValueType,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};
pub use wasmer_vm::{raise_user_trap, Export, MemoryError};
#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

// The compilers are mutually exclusive
#[cfg(any(
    all(
        feature = "default-llvm",
        any(feature = "default-cranelift", feature = "default-singlepass")
    ),
    all(feature = "default-cranelift", feature = "default-singlepass")
))]
compile_error!(
    r#"The `default-singlepass`, `default-cranelift` and `default-llvm` features are mutually exclusive.
If you wish to use more than one compiler, you can simply create the own store. Eg.:

```
use wasmer::{Store, JIT, Singlepass};

let engine = JIT::new(&Singlepass::default()).engine();
let store = Store::new(&engine);
```"#
);

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::Cranelift;

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::LLVM;

#[cfg(feature = "jit")]
pub use wasmer_engine_jit::{JITArtifact, JITEngine, JIT};

#[cfg(feature = "native")]
pub use wasmer_engine_native::{Native, NativeArtifact, NativeEngine};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
