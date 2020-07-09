//! This are the common types and utility tools for using WebAssembly
//! in a Rust environment.
//!
//! This crate provides common structures such as `Type` or `Value`, type indexes
//! and native function wrappers with `Func`.

#![deny(missing_docs, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "clippy", plugin(clippy(conf_file = "../../clippy.toml")))]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
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

mod data_initializer;
mod features;
mod indexes;
mod memory_view;
mod native;
mod r#ref;
mod types;
mod units;
mod values;

/// The entity module, with common helpers for Rust structures
pub mod entity {
    pub use cranelift_entity::*;
}

pub use crate::data_initializer::{DataInitializer, DataInitializerLocation, OwnedDataInitializer};
pub use crate::features::Features;
pub use crate::indexes::{
    CustomSectionIndex, DataIndex, ElemIndex, ExportIndex, FunctionIndex, GlobalIndex, ImportIndex,
    LocalFunctionIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex,
    SignatureIndex, TableIndex,
};
pub use crate::memory_view::{Atomically, MemoryView};
pub use crate::native::{NativeWasmType, ValueType};
pub use crate::r#ref::{ExternRef, HostInfo, HostRef};
pub use crate::units::{Bytes, Pages, WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
pub use crate::values::Value;
pub use types::{
    ExportType, ExternType, FunctionType, GlobalInit, GlobalType, ImportType, MemoryType,
    Mutability, TableType, Type, V128,
};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
