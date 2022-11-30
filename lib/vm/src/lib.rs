//! Runtime library support for Wasmer.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![deny(trivial_numeric_casts, unused_extern_crates)]
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

mod export;
mod extern_ref;
mod function_env;
mod global;
mod imports;
mod instance;
mod memory;
mod mmap;
mod probestack;
mod sig_registry;
mod store;
mod table;
mod trap;
mod vmcontext;

pub mod libcalls;

use std::ptr::NonNull;

pub use crate::export::*;
pub use crate::extern_ref::{VMExternObj, VMExternRef};
pub use crate::function_env::VMFunctionEnvironment;
pub use crate::global::*;
pub use crate::imports::Imports;
pub use crate::instance::{InstanceAllocator, InstanceHandle};
pub use crate::memory::{
    initialize_memory_with_data, LinearMemory, VMMemory, VMOwnedMemory, VMSharedMemory,
};
pub use crate::mmap::Mmap;
pub use crate::probestack::PROBESTACK;
pub use crate::sig_registry::SignatureRegistry;
pub use crate::store::{
    InternalStoreHandle, MaybeInstanceOwned, StoreHandle, StoreId, StoreObjects,
};
pub use crate::table::{TableElement, VMTable};
pub use crate::trap::*;
pub use crate::vmcontext::{
    VMCallerCheckedAnyfunc, VMContext, VMDynamicFunctionContext, VMFunctionContext,
    VMFunctionImport, VMFunctionKind, VMGlobalDefinition, VMGlobalImport, VMMemoryDefinition,
    VMMemoryImport, VMSharedSignatureIndex, VMTableDefinition, VMTableImport, VMTrampoline,
};
pub use wasmer_types::LibCall;
pub use wasmer_types::MemoryError;
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

/// A placeholder byte-sized type which is just used to provide some amount of type
/// safety when dealing with pointers to JIT-compiled function bodies. Note that it's
/// deliberately not Copy, as we shouldn't be carelessly copying function body bytes
/// around.
#[repr(C)]
pub struct VMFunctionBody(u8);

#[cfg(test)]
mod test_vmfunction_body {
    use super::VMFunctionBody;
    use std::mem::size_of;

    #[test]
    fn check_vmfunction_body_offsets() {
        assert_eq!(size_of::<VMFunctionBody>(), 1);
    }
}

/// A safe wrapper around `VMFunctionBody`.
#[derive(Clone, Copy, Debug)]
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
    ///
    /// # Safety
    /// `raw.funcref` must be a valid pointer.
    pub unsafe fn from_raw(raw: RawValue) -> Option<Self> {
        NonNull::new(raw.funcref as *mut VMCallerCheckedAnyfunc).map(Self)
    }
}
