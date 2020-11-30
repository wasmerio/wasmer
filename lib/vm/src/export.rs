// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use crate::global::Global;
use crate::memory::{Memory, MemoryStyle};
use crate::table::{Table, TableStyle};
use crate::vmcontext::{VMFunctionBody, VMFunctionEnvironment, VMFunctionKind, VMTrampoline};
use std::sync::Arc;
use wasmer_types::{FunctionType, MemoryType, TableType};

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum VMExport {
    /// A function export value.
    Function(VMExportFunction),

    /// A table export value.
    Table(VMExportTable),

    /// A memory export value.
    Memory(VMExportMemory),

    /// A global export value.
    Global(VMExportGlobal),
}

/// A function export value.
#[derive(Debug, Clone, PartialEq)]
pub struct VMExportFunction {
    /// The address of the native-code function.
    pub address: *const VMFunctionBody,
    /// Pointer to the containing `VMContext`.
    pub vmctx: VMFunctionEnvironment,
    /// The function type, used for compatibility checking.
    pub signature: FunctionType,
    /// The function kind (specifies the calling convention for the function).
    pub kind: VMFunctionKind,
    /// Address of the function call trampoline owned by the same VMContext that owns the VMFunctionBody.
    /// May be None when the function is a host-function (FunctionType == Dynamic or vmctx == nullptr).
    pub call_trampoline: Option<VMTrampoline>,
}

/// # Safety
/// There is no non-threadsafe logic directly in this type. Calling the function
/// may not be threadsafe.
unsafe impl Send for VMExportFunction {}
/// # Safety
/// The members of an VMExportFunction are immutable after construction.
unsafe impl Sync for VMExportFunction {}

impl From<VMExportFunction> for VMExport {
    fn from(func: VMExportFunction) -> Self {
        Self::Function(func)
    }
}

/// A table export value.
#[derive(Debug, Clone)]
pub struct VMExportTable {
    /// Pointer to the containing `Table`.
    pub from: Arc<dyn Table>,
}

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw table from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for VMExportTable {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
/// only makes this type easier to use)
unsafe impl Sync for VMExportTable {}

impl VMExportTable {
    /// Get the table type for this exported table
    pub fn ty(&self) -> &TableType {
        self.from.ty()
    }

    /// Get the style for this exported table
    pub fn style(&self) -> &TableStyle {
        self.from.style()
    }

    /// Returns whether or not the two `VMExportTable`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<VMExportTable> for VMExport {
    fn from(table: VMExportTable) -> Self {
        Self::Table(table)
    }
}

/// A memory export value.
#[derive(Debug, Clone)]
pub struct VMExportMemory {
    /// Pointer to the containing `Memory`.
    pub from: Arc<dyn Memory>,
}

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw memory from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for VMExportMemory {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
/// only makes this type easier to use)
unsafe impl Sync for VMExportMemory {}

impl VMExportMemory {
    /// Get the type for this exported memory
    pub fn ty(&self) -> &MemoryType {
        self.from.ty()
    }

    /// Get the style for this exported memory
    pub fn style(&self) -> &MemoryStyle {
        self.from.style()
    }

    /// Returns whether or not the two `VMExportMemory`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<VMExportMemory> for VMExport {
    fn from(memory: VMExportMemory) -> Self {
        Self::Memory(memory)
    }
}

/// A global export value.
#[derive(Debug, Clone)]
pub struct VMExportGlobal {
    /// The global declaration, used for compatibility checking.
    pub from: Arc<Global>,
}

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw global from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for VMExportGlobal {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// from the perspective of users of this type and the type is both `Send` and `Clone` (thus
/// marking it `Sync` adds no new behavior, it only makes this type easier to use)
unsafe impl Sync for VMExportGlobal {}

impl VMExportGlobal {
    /// Returns whether or not the two `VMExportGlobal`s refer to the same Global.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<VMExportGlobal> for VMExport {
    fn from(global: VMExportGlobal) -> Self {
        Self::Global(global)
    }
}
