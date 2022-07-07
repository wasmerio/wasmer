// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use crate::global::VMGlobal;
use crate::memory::VMMemory;
use crate::store::InternalStoreHandle;
use crate::table::VMTable;
use crate::vmcontext::VMFunctionKind;
use crate::{MaybeInstanceOwned, VMCallerCheckedAnyfunc};
use std::any::Any;
use wasmer_types::FunctionType;

/// The value of an export passed from one instance to another.
pub enum VMExtern {
    /// A function export value.
    Function(InternalStoreHandle<VMFunction>),

    /// A table export value.
    Table(InternalStoreHandle<VMTable>),

    /// A memory export value.
    Memory(InternalStoreHandle<VMMemory>),

    /// A global export value.
    Global(InternalStoreHandle<VMGlobal>),
}

/// A function export value.
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
