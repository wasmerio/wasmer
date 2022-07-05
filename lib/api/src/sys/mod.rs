mod env;
mod exports;
mod externals;
mod imports;
mod instance;
mod mem_access;
mod module;
mod native;
mod ptr;
mod store;
mod tunables;
mod types;

/// Implement [`WasmerEnv`] for your type with `#[derive(WasmerEnv)]`.
///
/// See the [`WasmerEnv`] trait for more information.
pub use wasmer_derive::WasmerEnv;

#[doc(hidden)]
pub mod internals {
    //! We use the internals module for exporting types that are only
    //! intended to use in internal crates such as the compatibility crate
    //! `wasmer-vm`. Please don't use any of this types directly, as
    //! they might change frequently or be removed in the future.

    pub use crate::sys::externals::{WithEnv, WithoutEnv};
}

pub use crate::sys::env::{HostEnvInitError, LazyInit, WasmerEnv};
pub use crate::sys::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::sys::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, Table, WasmTypeList,
};
pub use crate::sys::imports::Imports;
pub use crate::sys::instance::{Instance, InstantiationError};
pub use crate::sys::mem_access::{MemoryAccessError, WasmRef, WasmSlice, WasmSliceIter};
pub use crate::sys::module::Module;
pub use crate::sys::native::TypedFunction;

pub use crate::sys::ptr::{Memory32, Memory64, MemorySize, WasmPtr, WasmPtr64};
pub use crate::sys::store::{Store, StoreObject};
pub use crate::sys::tunables::BaseTunables;
pub use crate::sys::types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Val, ValType,
};
pub use crate::sys::types::{Val as Value, ValType as Type};
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};
pub use wasmer_compiler::{
    CpuFeature, Engine, Export, Features, FrameInfo, LinkError, RuntimeError, Target, Tunables,
};
pub use wasmer_derive::ValueType;
pub use wasmer_types::is_wasm;
#[cfg(feature = "experimental-reference-types-extern-ref")]
pub use wasmer_types::ExternRef;
pub use wasmer_types::{
    Bytes, CompileError, DeserializeError, ExportIndex, GlobalInit, LocalFunctionIndex,
    MiddlewareError, Pages, ParseCpuFeatureError, SerializeError, ValueType, WasmError, WasmResult,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

// TODO: should those be moved into wasmer::vm as well?
pub use wasmer_vm::{raise_user_trap, MemoryError};
pub mod vm {
    //! The `vm` module re-exports wasmer-vm types.

    pub use wasmer_vm::{
        Memory, MemoryError, MemoryStyle, Table, TableStyle, VMExtern, VMMemoryDefinition,
        VMTableDefinition,
    };
}

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
use wasmer::{Store, Universal, Singlepass};

let engine = Universal::new(Singlepass::default()).engine();
let store = Store::new_with_engine(&engine);
```"#
);

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};

#[cfg(feature = "compiler")]
pub use wasmer_compiler::{Universal, UniversalArtifact, UniversalEngine};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Deprecated JIT Engine (please use `Universal` instead)
#[cfg(feature = "jit")]
#[deprecated(since = "2.0.0", note = "Please use the `universal` feature instead")]
pub type JIT = Universal;

/// This type is deprecated, it has been replaced by TypedFunction.
#[deprecated(
    since = "3.0.0",
    note = "NativeFunc has been replaced by TypedFunction"
)]
pub type NativeFunc<Args = (), Rets = ()> = TypedFunction<Args, Rets>;
