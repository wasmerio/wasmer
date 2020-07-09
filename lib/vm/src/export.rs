// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer-reborn/blob/master/ATTRIBUTIONS.md

use crate::memory::{Memory, MemoryStyle};
use crate::table::{Table, TableStyle};
use crate::vmcontext::{VMContext, VMFunctionBody, VMFunctionKind, VMGlobalDefinition};
use std::sync::Arc;
use wasm_common::{FunctionType, GlobalType, MemoryType, TableType};

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum Export {
    /// A function export value.
    Function(ExportFunction),

    /// A table export value.
    Table(ExportTable),

    /// A memory export value.
    Memory(ExportMemory),

    /// A global export value.
    Global(ExportGlobal),
}

/// A function export value.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFunction {
    /// The address of the native-code function.
    pub address: *const VMFunctionBody,
    /// Pointer to the containing `VMContext`.
    pub vmctx: *mut VMContext,
    /// The function type, used for compatibility checking.
    pub signature: FunctionType,
    /// The function kind (it defines how it's the signature that provided `address` have)
    pub kind: VMFunctionKind,
}

impl From<ExportFunction> for Export {
    fn from(func: ExportFunction) -> Self {
        Self::Function(func)
    }
}

/// A table export value.
#[derive(Debug, Clone)]
pub struct ExportTable {
    /// Pointer to the containing `Table`.
    pub from: Arc<dyn Table>,
}

// This is correct because there is no non-threadsafe logic directly in this type;
// correct use of the raw table from multiple threads via `definition` requires `unsafe`
// and is the responsibilty of the user of this type.
unsafe impl Send for ExportTable {}
// This is correct because the values directly in `definition` should be considered immutable
// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
// only makes this type easier to use)
unsafe impl Sync for ExportTable {}

impl ExportTable {
    /// Get the table type for this exported table
    pub fn ty(&self) -> &TableType {
        self.from.ty()
    }

    /// Get the style for this exported table
    pub fn style(&self) -> &TableStyle {
        self.from.style()
    }

    /// Returns whether or not the two `ExportTable`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<ExportTable> for Export {
    fn from(table: ExportTable) -> Self {
        Self::Table(table)
    }
}

/// A memory export value.
#[derive(Debug, Clone)]
pub struct ExportMemory {
    /// Pointer to the containing `Memory`.
    pub from: Arc<dyn Memory>,
}

// This is correct because there is no non-threadsafe logic directly in this type;
// correct use of the raw memory from multiple threads via `definition` requires `unsafe`
// and is the responsibilty of the user of this type.
unsafe impl Send for ExportMemory {}
// This is correct because the values directly in `definition` should be considered immutable
// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
// only makes this type easier to use)
unsafe impl Sync for ExportMemory {}

impl ExportMemory {
    /// Get the type for this exported memory
    pub fn ty(&self) -> &MemoryType {
        self.from.ty()
    }

    /// Get the style for this exported memory
    pub fn style(&self) -> &MemoryStyle {
        self.from.style()
    }

    /// Returns whether or not the two `ExportMemory`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<ExportMemory> for Export {
    fn from(memory: ExportMemory) -> Self {
        Self::Memory(memory)
    }
}

/// A global export value.
#[derive(Debug, Clone)]
pub struct ExportGlobal {
    /// The address of the global storage.
    pub definition: *mut VMGlobalDefinition,
    /// The global declaration, used for compatibility checking.
    pub global: GlobalType,
}

impl ExportGlobal {
    /// Returns whether or not the two `ExportGlobal`s refer to the same Global.
    pub fn same(&self, other: &Self) -> bool {
        self.definition == other.definition && self.global == other.global
    }
}

impl From<ExportGlobal> for Export {
    fn from(global: ExportGlobal) -> Self {
        Self::Global(global)
    }
}
