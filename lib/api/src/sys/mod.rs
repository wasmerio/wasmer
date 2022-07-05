mod context;
mod exports;
mod extern_ref;
mod externals;
mod imports;
mod instance;
mod mem_access;
mod module;
mod native;
mod native_type;
mod ptr;
mod store;
mod tunables;
mod value;

pub use crate::sys::context::{AsContextMut, AsContextRef, Context, ContextMut, ContextRef};
pub use crate::sys::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::sys::extern_ref::ExternRef;
pub use crate::sys::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, Table, WasmTypeList,
};
pub use crate::sys::imports::Imports;
pub use crate::sys::instance::{Instance, InstantiationError};
pub use crate::sys::mem_access::{MemoryAccessError, WasmRef, WasmSlice, WasmSliceIter};
pub use crate::sys::module::Module;
pub use crate::sys::native::TypedFunction;
pub use crate::sys::native_type::NativeWasmTypeInto;

pub use crate::sys::ptr::{Memory32, Memory64, MemorySize, WasmPtr, WasmPtr64};
pub use crate::sys::store::Store;
pub use crate::sys::tunables::BaseTunables;
pub use crate::sys::value::Value;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};
pub use wasmer_compiler::{Features, FrameInfo, LinkError, RuntimeError, Tunables};
pub use wasmer_derive::ValueType;
pub use wasmer_types::is_wasm;
pub use wasmer_types::{
    CpuFeature, ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType,
    Mutability, TableType, Target, Type,
};

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
        MemoryError, MemoryStyle, TableStyle, VMExtern, VMMemory, VMMemoryDefinition, VMTable,
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

#[cfg(feature = "universal")]
pub use wasmer_compiler::{Artifact, Engine, Universal};

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
