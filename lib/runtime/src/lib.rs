//! Runtime library support for Wasmer.

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

mod export;
mod imports;
mod instance;
mod memory;
mod mmap;
mod module;
mod probestack;
mod sig_registry;
mod table;
mod trap;
mod vmcontext;
mod vmoffsets;

pub mod libcalls;

pub use crate::export::*;
pub use crate::imports::Imports;
pub use crate::instance::InstanceHandle;
pub use crate::memory::LinearMemory;
pub use crate::mmap::Mmap;
pub use crate::module::{MemoryPlan, MemoryStyle, Module, TableElements, TablePlan, TableStyle};
pub use crate::probestack::PROBESTACK;
pub use crate::sig_registry::SignatureRegistry;
pub use crate::table::Table;
pub use crate::trap::*;
pub use crate::vmcontext::{
    VMBuiltinFunctionIndex, VMCallerCheckedAnyfunc, VMContext, VMFunctionBody, VMFunctionImport,
    VMGlobalDefinition, VMGlobalImport, VMMemoryDefinition, VMMemoryImport, VMSharedSignatureIndex,
    VMTableDefinition, VMTableImport, VMTrampoline,
};
pub use crate::vmoffsets::{TargetSharedSignatureIndex, VMOffsets};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
