//! This are the common types and utility tools for using WebAssembly
//! in a Rust environment.
//!
//! This crate provides common structures such as `Type` or `Value`, type indexes
//! and native function wrappers with `Func`.

#![deny(missing_docs, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "std", deny(unstable_features))]
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

mod constants;
mod data_initializer;
mod features;
mod indexes;
mod native;
mod r#ref;
mod types;
mod values;

/// The entity module, with common helpers for Rust structures
pub mod entity {
    pub use cranelift_entity::*;
}

pub use crate::constants::*;
pub use crate::data_initializer::{DataInitializer, DataInitializerLocation};
pub use crate::features::Features;
pub use crate::indexes::{
    DataIndex, DefinedFuncIndex, DefinedGlobalIndex, DefinedMemoryIndex, DefinedTableIndex,
    ElemIndex, ExportIndex, FuncIndex, GlobalIndex, ImportIndex, MemoryIndex, SignatureIndex,
    TableIndex,
};
pub use crate::native::{Func, HostFunction, NativeWasmType, WasmTypeList, WithEnv, WithoutEnv};
pub use crate::r#ref::{AnyRef, HostInfo, HostRef};
pub use crate::values::Value;
pub use types::{
    ExportType, ExternType, FuncType, GlobalInit, GlobalType, ImportType, MemoryType, Mutability,
    TableType, Type, V128,
};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
