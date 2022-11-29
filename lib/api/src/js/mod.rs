#[cfg(all(feature = "std", feature = "core"))]
compile_error!(
    "The `std` and `core` features are both enabled, which is an error. Please enable only once."
);

#[cfg(all(not(feature = "std"), not(feature = "core")))]
compile_error!("Both the `std` and `core` features are disabled. Please enable one of them.");

#[cfg(feature = "core")]
pub(crate) extern crate alloc;

mod lib {
    #[cfg(feature = "core")]
    pub mod std {
        pub use crate::alloc::{borrow, boxed, str, string, sync, vec};
        pub use core::fmt;
        pub use hashbrown as collections;
    }

    #[cfg(feature = "std")]
    pub mod std {
        pub use std::{borrow, boxed, collections, fmt, str, string, sync, vec};
    }
}

pub(crate) mod error;
mod export;
mod exports;
mod externals;
mod function_env;
mod imports;
mod instance;
mod mem_access;
mod module;
#[cfg(feature = "wasm-types-polyfill")]
mod module_info_polyfill;
mod native;
mod native_type;
mod ptr;
mod store;
mod trap;
mod types;
mod value;
mod wasm_bindgen_polyfill;

pub use crate::js::error::{DeserializeError, InstantiationError, SerializeError};
pub use crate::js::export::Export;
pub use crate::js::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::js::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, MemoryError, MemoryView,
    Table, WasmTypeList,
};
pub use crate::js::function_env::{FunctionEnv, FunctionEnvMut};
pub use crate::js::imports::Imports;
pub use crate::js::instance::Instance;
pub use crate::js::mem_access::{MemoryAccessError, WasmRef, WasmSlice, WasmSliceIter};
pub use crate::js::module::{IoCompileError, Module, ModuleTypeHints};
pub use crate::js::native::TypedFunction;
pub use crate::js::native_type::NativeWasmTypeInto;
pub use crate::js::ptr::{Memory32, Memory64, MemorySize, WasmPtr, WasmPtr64};
pub use crate::js::trap::RuntimeError;

pub use crate::js::store::{
    AsStoreMut, AsStoreRef, Store, StoreHandle, StoreMut, StoreObject, StoreObjects, StoreRef,
};
pub use crate::js::types::ValType as Type;
pub use crate::js::types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, ValType,
};
pub use crate::js::value::Value;
pub use crate::js::value::Value as Val;

pub mod vm {
    //! The `vm` module re-exports wasmer-vm types.
    pub use crate::js::export::VMMemory;
}

pub use wasmer_types::is_wasm;
pub use wasmer_types::{
    Bytes, ExportIndex, GlobalInit, LocalFunctionIndex, Pages, ValueType, WASM_MAX_PAGES,
    WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

#[cfg(feature = "wasm-types-polyfill")]
pub use wasmparser;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// This type is deprecated, it has been replaced by TypedFunction.
#[deprecated(
    since = "3.0.0",
    note = "NativeFunc has been replaced by TypedFunction"
)]
pub type NativeFunc<Args = (), Rets = ()> = TypedFunction<Args, Rets>;
