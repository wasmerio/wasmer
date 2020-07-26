//! This are the common types and utility tools for using WebAssembly
//! in a Rust environment.
//!
//! This crate provides common structures such as `Type` or `Value`, type indexes
//! and native function wrappers with `Func`.

#![deny(missing_docs, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "std", deny(unstable_features))]
#![cfg_attr(not(feature = "std"), no_std)]
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
        pub use alloc::{borrow, boxed, format, rc, slice, string, vec};
        pub use core::{any, cell, fmt, hash, marker, ops, ptr, sync};
    }

    #[cfg(feature = "std")]
    pub mod std {
        pub use std::{
            any, borrow, boxed, cell, fmt, format, hash, marker, ops, ptr, rc, slice, string, sync,
            vec,
        };
    }
}

mod features;
mod indexes;
mod initializers;
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

pub use crate::features::Features;
pub use crate::indexes::{
    CustomSectionIndex, DataIndex, ElemIndex, ExportIndex, FunctionIndex, GlobalIndex, ImportIndex,
    LocalFunctionIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex,
    SignatureIndex, TableIndex,
};
pub use crate::initializers::{
    DataInitializer, DataInitializerLocation, OwnedDataInitializer, TableInitializer,
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
