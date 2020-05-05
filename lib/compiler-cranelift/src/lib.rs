//! A WebAssembly `Compiler` implementation using Cranelift.
//!
//! Cranelift is a fast IR generator created by Mozilla for usage in
//! Firefox as a next JS compiler generator.
//!
//! Compared to LLVM, Cranelit is a bit faster and made enterely in Rust.
#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "clippy", plugin(clippy(conf_file = "../../clippy.toml")))]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, clippy::new_without_default)
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

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc as std;
#[cfg(feature = "std")]
#[macro_use]
extern crate std;

#[cfg(not(feature = "std"))]
use hashbrown::{
    hash_map,
    hash_map::Entry::{Occupied, Vacant},
    HashMap,
};
#[cfg(feature = "std")]
use std::collections::{
    hash_map,
    hash_map::Entry::{Occupied, Vacant},
    HashMap,
};

mod address_map;
mod compiler;
mod config;
mod debug;
mod func_environ;
mod sink;
mod trampoline;
mod translator;

pub use crate::compiler::CraneliftCompiler;
pub use crate::config::CraneliftConfig;
pub use crate::debug::{FrameLayout, FrameLayoutChange, FrameLayouts};
pub use crate::debug::{ModuleMemoryOffset, ModuleVmctxInfo, ValueLabelsRanges};
pub use crate::trampoline::make_wasm_trampoline;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
