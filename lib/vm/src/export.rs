// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

use crate::global::VMGlobal;
use crate::memory::VMMemory;
use crate::store::InternalStoreHandle;
use crate::table::VMTable;
use crate::vmcontext::VMFunctionKind;
use crate::{MaybeInstanceOwned, VMCallerCheckedAnyfunc};
use std::any::Any;
use wasmer_types::{FunctionType, TagKind};

/// The value of an export passed from one instance to another.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub enum VMExtern {
    /// A function export value.
    Function(InternalStoreHandle<VMFunction>),

    /// A table export value.
    Table(InternalStoreHandle<VMTable>),

    /// A memory export value.
    Memory(InternalStoreHandle<VMMemory>),

    /// A global export value.
    Global(InternalStoreHandle<VMGlobal>),

    /// A tag export value.
    Tag(InternalStoreHandle<VMTag>),
}

/// A function export value.
#[derive(Debug)]
pub struct VMFunction {
    /// Pointer to the `VMCallerCheckedAnyfunc` which contains data needed to
    /// call the function and check its signature.
    pub anyfunc: MaybeInstanceOwned<VMCallerCheckedAnyfunc>,

    /// The function type, used for compatibility checking.
    pub signature: FunctionType,

    /// The function kind (specifies the calling convention for the
    /// function).
    pub kind: VMFunctionKind,

    /// Associated data owned by a host function.
    pub host_data: Box<dyn Any>,
}

/// A tag export value.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VMTag {
    /// The kind of tag.
    // Note: currently it can only be exception.
    pub kind: TagKind,
    /// The tag type, used for compatibility checking.
    pub signature: FunctionType,
}

impl VMTag {
    /// Create a new [`VMTag`].
    pub fn new(kind: TagKind, signature: FunctionType) -> Self {
        Self { kind, signature }
    }
}
