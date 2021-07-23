#![doc(
    html_logo_url = "https://github.com/wasmerio.png?size=200",
    html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png"
)]
#![deny(
    missing_docs,
    trivial_numeric_casts,
    unused_extern_crates,
    broken_intra_doc_links
)]
#![warn(unused_import_braces)]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, clippy::vtable_address_comparisons)
)]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::map_unwrap_or,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

//! This crate contains the `wasmer-js` API. The `wasmer-js` API facilitates the efficient,
//! sandboxed execution of [WebAssembly (Wasm)][wasm] modules, leveraging on the same
//! API as the `wasmer` crate, but targeting Javascript.
//!
//! This crate uses the same WebAssembly engine as the Javascript VM where it's used.
//!
//! Here's an example of the `wasmer-js` API in action:
//! ```
//! #[wasm_bindgen]
//! pub extern fn do_add_one_in_wasmer() -> i32 {
//!     let module_wat = r#"
//!     (module
//!     (type $t0 (func (param i32) (result i32)))
//!     (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
//!         get_local $p0
//!         i32.const 1
//!         i32.add))
//!     "#;
//!     let store = Store::default();
//!     let module = Module::new(&store, &module_wat).unwrap();
//!     // The module doesn't import anything, so we create an empty import object.
//!     let import_object = imports! {};
//!     let instance = Instance::new(&module, &import_object).unwrap();
//!     let add_one = instance.exports.get_function("add_one").unwrap();
//!     let result = add_one.call(&[Value::I32(42)]).unwrap();
//!     assert_eq!(result[0], Value::I32(43));
//!     result[0].unwrap_i32()
//! }
//! ```
//!
//! For more examples of using the `wasmer` API, check out the
//! [wasmer examples][wasmer-examples].

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
mod module;
#[cfg(feature = "wasm-types-polyfill")]
mod module_info_polyfill;
mod native;
mod ptr;
mod resolver;
mod store;
mod trap;
mod types;
mod utils;
mod wasm_bindgen_polyfill;

/// Implement [`WasmerEnv`] for your type with `#[derive(WasmerEnv)]`.
///
/// See the [`WasmerEnv`] trait for more information.
pub use wasmer_derive::WasmerEnv;

pub use crate::cell::WasmCell;
pub use crate::env::{HostEnvInitError, LazyInit, WasmerEnv};
pub use crate::exports::{ExportError, Exportable, Exports, ExportsIterator};
pub use crate::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, MemoryError, Table,
    WasmTypeList,
};
pub use crate::import_object::{ImportObject, ImportObjectIterator, LikeNamespace};
pub use crate::instance::{Instance, InstantiationError};
pub use crate::module::{Module, ModuleTypeHints};
pub use crate::native::NativeFunc;
pub use crate::ptr::{Array, Item, WasmPtr};
pub use crate::resolver::{ChainableNamedResolver, NamedResolver, NamedResolverChain, Resolver};
pub use crate::trap::RuntimeError;

pub use crate::store::{Store, StoreObject};
pub use crate::types::{
    ExportType, ExternType, FunctionType, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Val, ValType,
};
pub use crate::types::{Val as Value, ValType as Type};
pub use crate::utils::is_wasm;

pub use wasmer_types::{
    Atomically, Bytes, ExportIndex, GlobalInit, LocalFunctionIndex, MemoryView, Pages, ValueType,
    WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE,
};

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
