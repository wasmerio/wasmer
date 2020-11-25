// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use crate::global::Global;
use crate::instance::ImportInitializerFuncPtr;
use crate::memory::{Memory, MemoryStyle};
use crate::table::{Table, TableStyle};
use crate::vmcontext::{VMFunctionBody, VMFunctionEnvironment, VMFunctionKind, VMTrampoline};
use std::sync::Arc;
use wasmer_types::{FunctionType, MemoryType, TableType};

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum EngineExport {
    /// A function export value.
    Function(EngineExportFunction),

    /// A table export value.
    Table(ExportTable),

    /// A memory export value.
    Memory(ExportMemory),

    /// A global export value.
    Global(ExportGlobal),
}

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

/// TODO:
#[derive(Debug, Clone, PartialEq)]
pub struct EngineExportFunction {
    /// TODO:
    pub function: ExportFunction,
    /// Function pointer to `WasmerEnv::init_with_instance(&mut self, instance: &Instance)`.
    ///
    /// This function is called to finish setting up the environment after
    /// we create the `api::Instance`.
    pub function_ptr: Option<ImportInitializerFuncPtr>,
}

/// A function export value.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFunction {
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
unsafe impl Send for ExportFunction {}
/// # Safety
/// The members of an ExportFunction are immutable after construction.
unsafe impl Sync for ExportFunction {}

impl From<EngineExportFunction> for EngineExport {
    fn from(func: EngineExportFunction) -> Self {
        Self::Function(func)
    }
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

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw table from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for ExportTable {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
/// only makes this type easier to use)
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

impl From<ExportTable> for EngineExport {
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

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw memory from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for ExportMemory {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// and the type is both `Send` and `Clone` (thus marking it `Sync` adds no new behavior, it
/// only makes this type easier to use)
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

impl From<ExportMemory> for EngineExport {
    fn from(memory: ExportMemory) -> Self {
        Self::Memory(memory)
    }
}

/// A global export value.
#[derive(Debug, Clone)]
pub struct ExportGlobal {
    /// The global declaration, used for compatibility checking.
    pub from: Arc<Global>,
}

/// # Safety
/// This is correct because there is no non-threadsafe logic directly in this type;
/// correct use of the raw global from multiple threads via `definition` requires `unsafe`
/// and is the responsibilty of the user of this type.
unsafe impl Send for ExportGlobal {}
/// # Safety
/// This is correct because the values directly in `definition` should be considered immutable
/// from the perspective of users of this type and the type is both `Send` and `Clone` (thus
/// marking it `Sync` adds no new behavior, it only makes this type easier to use)
unsafe impl Sync for ExportGlobal {}

impl ExportGlobal {
    /// Returns whether or not the two `ExportGlobal`s refer to the same Global.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.from, &other.from)
    }
}

impl From<ExportGlobal> for Export {
    fn from(global: ExportGlobal) -> Self {
        Self::Global(global)
    }
}

impl From<ExportGlobal> for EngineExport {
    fn from(global: ExportGlobal) -> Self {
        Self::Global(global)
    }
}
