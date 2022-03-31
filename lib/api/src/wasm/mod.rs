#[cfg(all(feature = "std", feature = "core"))]
compile_error!(
    "The `std` and `core` features are both enabled, which is an error. Please enable only once."
);

#[cfg(all(not(feature = "std"), not(feature = "core")))]
compile_error!("Both the `std` and `core` features are disabled. Please enable one of them.");

#[cfg(feature = "core")]
extern crate alloc;

mod lib {
    #[cfg(feature = "core")]
    pub mod std {
        pub use alloc::{borrow, boxed, str, string, sync, vec};
        pub use core::fmt;
        pub use hashbrown as collections;
    }

    #[cfg(feature = "std")]
    pub mod std {
        pub use std::{borrow, boxed, collections, fmt, str, string, sync, vec};
    }
}

mod cell;
mod env;
mod error;
mod export;
mod exports;
mod externals;
mod import_object;
mod instance;
//mod js_import_object;
mod module;
mod native;
mod ptr;
mod resolver;
mod store;
mod trap;
mod tunables;
mod types;
mod utils;

/// Implement [`WasmerEnv`] for your type with `#[derive(WasmerEnv)]`.
///
/// See the [`WasmerEnv`] trait for more information.
pub use wasmer_derive::WasmerEnv;

pub use crate::wasm::cell::WasmCell;
pub use crate::wasm::env::{HostEnvInitError, LazyInit, WasmerEnv};
pub use crate::wasm::error::{DeserializeError, SerializeError};
pub use crate::wasm::export::Export;
pub use crate::wasm::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::wasm::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, Table, WasmTypeList,
};
pub use crate::wasm::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::wasm::instance::{Instance, InstantiationError};
//pub use crate::wasm::js_import_object::JsImportObject;
pub use crate::wasm::module::Module;
pub use crate::wasm::native::NativeFunc;
pub use crate::wasm::ptr::{Array, Item, WasmPtr};
pub use crate::wasm::resolver::{
    ChainableNamedResolver, NamedResolver, NamedResolverChain, Resolver,
};
pub use crate::wasm::trap::RuntimeError;
pub use crate::wasm::tunables::BaseTunables;

pub use crate::wasm::store::{Store, StoreObject};
pub use crate::wasm::types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Val, ValType,
};
pub use crate::wasm::types::{Val as Value, ValType as Type};
pub use crate::wasm::utils::is_wasm;

pub use wasmer_types::{
    Atomically, Bytes, ExportIndex, GlobalInit, LocalFunctionIndex, MemoryView, Pages, ValueType,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

pub use wasmer_fakevm::{raise_user_trap, MemoryError};
pub mod fakevm {
    //! The `vm` module re-exports wasmer-vm types.

    pub use wasmer_fakevm::{
        Memory, MemoryError, MemoryStyle, Table, TableStyle, VMExtern, VMMemoryDefinition,
        VMTableDefinition,
    };
}

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
