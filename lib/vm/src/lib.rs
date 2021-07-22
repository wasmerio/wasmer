//! Runtime library support for Wasmer.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![deny(trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(clippy::new_without_default, vtable_address_comparisons)
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
mod func_data_registry;
mod global;
mod imports;
mod instance;
mod memory;
mod mmap;
mod probestack;
mod sig_registry;
mod table;
mod trap;
mod vmcontext;
mod vmoffsets;

pub mod libcalls;

pub use crate::export::*;
pub use crate::func_data_registry::{FuncDataRegistry, VMFuncRef};
pub use crate::global::*;
pub use crate::imports::Imports;
pub use crate::instance::{
    ImportFunctionEnv, ImportInitializerFuncPtr, InstanceAllocator, InstanceHandle,
    WeakOrStrongInstanceRef,
};
pub use crate::memory::{LinearMemory, Memory, MemoryError, MemoryStyle};
pub use crate::mmap::Mmap;
pub use crate::probestack::PROBESTACK;
pub use crate::sig_registry::SignatureRegistry;
pub use crate::table::{LinearTable, Table, TableElement, TableStyle};
pub use crate::trap::*;
pub use crate::vmcontext::{
    VMBuiltinFunctionIndex, VMCallerCheckedAnyfunc, VMContext, VMDynamicFunctionContext,
    VMFunctionBody, VMFunctionEnvironment, VMFunctionImport, VMFunctionKind, VMGlobalDefinition,
    VMGlobalImport, VMMemoryDefinition, VMMemoryImport, VMSharedSignatureIndex, VMTableDefinition,
    VMTableImport, VMTrampoline,
};
pub use crate::vmoffsets::{TargetSharedSignatureIndex, VMOffsets};
use loupe::MemoryUsage;
pub use wasmer_types::VMExternRef;
#[deprecated(
    since = "2.1.0",
    note = "ModuleInfo, ExportsIterator, ImportsIterator should be imported from wasmer_types."
)]
pub use wasmer_types::{ExportsIterator, ImportsIterator, ModuleInfo};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A safe wrapper around `VMFunctionBody`.
#[derive(Clone, Copy, Debug, MemoryUsage)]
#[repr(transparent)]
pub struct FunctionBodyPtr(pub *const VMFunctionBody);

impl std::ops::Deref for FunctionBodyPtr {
    type Target = *const VMFunctionBody;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// # Safety
/// The VMFunctionBody that this points to is opaque, so there's no data to
/// read or write through this pointer. This is essentially a usize.
unsafe impl Send for FunctionBodyPtr {}
/// # Safety
/// The VMFunctionBody that this points to is opaque, so there's no data to
/// read or write through this pointer. This is essentially a usize.
unsafe impl Sync for FunctionBodyPtr {}

/// Pointers to section data.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct SectionBodyPtr(pub *const u8);

impl std::ops::Deref for SectionBodyPtr {
    type Target = *const u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
