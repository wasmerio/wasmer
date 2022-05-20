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
        clippy::map_unwrap_or,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

mod context;
mod export;
mod extern_ref;
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

pub mod libcalls;

use std::ptr::NonNull;

pub use crate::context::{
    ContextHandle, ContextId, ContextObjects, InternalContextHandle, MaybeInstanceOwned,
};
pub use crate::export::*;
pub use crate::extern_ref::{VMExternObj, VMExternRef};
pub use crate::global::*;
pub use crate::imports::Imports;
pub use crate::instance::{InstanceAllocator, InstanceHandle};
pub use crate::memory::{MemoryError, VMMemory};
pub use crate::mmap::Mmap;
pub use crate::probestack::PROBESTACK;
pub use crate::sig_registry::SignatureRegistry;
pub use crate::table::{TableElement, VMTable};
pub use crate::trap::*;
pub use crate::vmcontext::{
    VMCallerCheckedAnyfunc, VMContext, VMDynamicFunctionContext, VMFunctionEnvironment,
    VMFunctionImport, VMFunctionKind, VMGlobalDefinition, VMGlobalImport, VMMemoryDefinition,
    VMMemoryImport, VMSharedSignatureIndex, VMTableDefinition, VMTableImport, VMTrampoline,
};
pub use wasmer_artifact::{FunctionBodyPtr, VMFunctionBody};
pub use wasmer_types::LibCall;
pub use wasmer_types::MemoryStyle;
use wasmer_types::RawValue;
pub use wasmer_types::TableStyle;
pub use wasmer_types::{TargetSharedSignatureIndex, VMBuiltinFunctionIndex, VMOffsets};

#[deprecated(
    since = "2.1.0",
    note = "ModuleInfo, ExportsIterator, ImportsIterator should be imported from wasmer_types."
)]
pub use wasmer_types::{ExportsIterator, ImportsIterator, ModuleInfo};

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

/// A function reference. A single word that points to metadata about a function.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VMFuncRef(pub NonNull<VMCallerCheckedAnyfunc>);

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        RawValue {
            funcref: self.0.as_ptr() as usize,
        }
    }

    /// Extracts a `VMFuncRef` from a `RawValue`.
    pub unsafe fn from_raw(raw: RawValue) -> Option<Self> {
        NonNull::new(raw.funcref as *mut VMCallerCheckedAnyfunc).map(Self)
    }
}
