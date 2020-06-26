// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer-reborn/blob/master/ATTRIBUTIONS.md

use crate::memory::{Memory, MemoryPlan};
use crate::table::{Table, TablePlan};
use crate::vmcontext::{
    VMContext, VMFunctionBody, VMFunctionKind, VMGlobalDefinition, VMMemoryDefinition,
    VMTableDefinition,
};
use std::ptr::NonNull;
use std::sync::Arc;
use wasm_common::{FunctionType, GlobalType};

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
    /// The function type, used for compatibilty checking.
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
    /// The address of the table descriptor.
    pub definition: NonNull<VMTableDefinition>,
    /// Pointer to the containing `Table`.
    pub from: Arc<dyn Table>,
}

impl ExportTable {
    /// Get the plan for this exported memory
    pub fn plan(&self) -> &TablePlan {
        self.from.plan()
    }

    /// Returns whether or not the two `ExportTable`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        // TODO: comparing
        self.definition == other.definition //&& self.from == other.from
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
    /// The address of the memory descriptor.
    pub definition: *mut VMMemoryDefinition,
    /// Pointer to the containing `Memory`.
    pub from: Arc<dyn Memory>,
}

impl ExportMemory {
    /// Get the plan for this exported memory
    pub fn plan(&self) -> &MemoryPlan {
        self.from.plan()
    }

    /// Returns whether or not the two `ExportMemory`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        // TODO: implement comparison
        self.definition == other.definition //&& self.from == other.from
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
