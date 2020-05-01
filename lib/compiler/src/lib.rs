//! The `wasmer-compiler` crate provides the necessary abstractions
//! to create a compiler.
//!
//! It provides an universal way of parsing a module via `wasmparser`,
//! while giving the responsibility of compiling specific function
//! WebAssembly bodies to the `Compiler` implementation.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
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
#![no_std]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc as std;
#[cfg(feature = "std")]
#[macro_use]
extern crate std;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

mod address_map;
mod compiler;
mod config;
mod error;
mod function;
mod jump_table;
mod relocation;
mod trap;
mod unwind;
#[macro_use]
mod translator;
mod section;
mod sourceloc;

pub use crate::address_map::{FunctionAddressMap, InstructionAddressMap};
pub use crate::compiler::Compiler;
pub use crate::config::{
    Architecture, CallingConvention, CompilerConfig, CpuFeature, Features, OperatingSystem, Target,
    Triple,
};
pub use crate::error::CompileError;
pub use crate::function::{Compilation, CompiledFunction, CustomSections, Functions};
pub use crate::jump_table::{JumpTable, JumpTableOffsets};
pub use crate::relocation::{Relocation, RelocationKind, RelocationTarget, Relocations};
pub use crate::section::{CustomSection, CustomSectionProtection, SectionIndex};
pub use crate::sourceloc::SourceLoc;
pub use crate::translator::{
    to_wasm_error, translate_module, FunctionBodyData, ModuleEnvironment, ModuleTranslation,
    ModuleTranslationState, WasmError, WasmResult,
};
pub use crate::trap::TrapInformation;
pub use crate::unwind::{CompiledFunctionUnwindInfo, FDERelocEntry, FunctionTableReloc};

/// wasmparser is exported as a module to slim compiler dependencies
pub mod wasmparser {
    pub use wasmparser::*;
}

/// Offset in bytes from the beginning of the function.
pub type CodeOffset = u32;

/// Addend to add to the symbol value.
pub type Addend = i64;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
