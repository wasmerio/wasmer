use crate::memory::LinearMemory;
use crate::module::{MemoryPlan, TablePlan};
use crate::table::Table;
use crate::vmcontext::{
    VMContext, VMFunctionBody, VMFunctionKind, VMGlobalDefinition, VMMemoryDefinition,
    VMTableDefinition,
};
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
    pub definition: *mut VMTableDefinition,
    /// Pointer to the containing `Table`.
    pub from: *mut Table,
}

impl ExportTable {
    /// Get the plan for this exported memory
    pub fn plan(&self) -> &TablePlan {
        unsafe { self.from.as_ref().unwrap() }.plan()
    }

    /// Returns whether or not the two `ExportTable`s refer to the same Memory.
    pub fn same(&self, other: &Self) -> bool {
        self.definition == other.definition && self.from == other.from
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
    /// Pointer to the containing `LinearMemory`.
    pub from: *mut LinearMemory,
}

impl ExportMemory {
    /// Get the plan for this exported memory
    pub fn plan(&self) -> &MemoryPlan {
        unsafe { self.from.as_ref().unwrap() }.plan()
    }

    /// Returns whether or not the two `ExportMemory`s refer to the same Memory.
    pub fn same(&self, other: &ExportMemory) -> bool {
        self.definition == other.definition && self.from == other.from
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
    pub fn same(&self, other: &ExportGlobal) -> bool {
        self.definition == other.definition && self.global == other.global
    }
}

impl From<ExportGlobal> for Export {
    fn from(global: ExportGlobal) -> Self {
        Self::Global(global)
    }
}
