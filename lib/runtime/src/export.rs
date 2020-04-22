use crate::memory::LinearMemory;
use crate::module::{MemoryPlan, TablePlan};
use crate::table::Table;
use crate::vmcontext::{
    VMContext, VMFunctionBody, VMGlobalDefinition, VMMemoryDefinition, VMSharedSignatureIndex,
    VMTableDefinition,
};
use wasm_common::GlobalType;

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
#[derive(Debug, Clone)]
pub struct ExportFunction {
    /// The address of the native-code function.
    pub address: *const VMFunctionBody,
    /// Pointer to the containing `VMContext`.
    pub vmctx: *mut VMContext,
    /// The function signature declaration, used for compatibilty checking.
    ///
    /// Note that this indexes within the module associated with `vmctx`.
    pub signature: VMSharedSignatureIndex,
}

impl From<ExportFunction> for Export {
    fn from(func: ExportFunction) -> Export {
        Export::Function(func)
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
}

impl From<ExportTable> for Export {
    fn from(table: ExportTable) -> Export {
        Export::Table(table)
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
}

impl From<ExportMemory> for Export {
    fn from(memory: ExportMemory) -> Export {
        Export::Memory(memory)
    }
}

/// A global export value.
#[derive(Debug, Clone)]
pub struct ExportGlobal {
    /// The address of the global storage.
    pub definition: *mut VMGlobalDefinition,
    /// The global declaration, used for compatibilty checking.
    pub global: GlobalType,
}

impl From<ExportGlobal> for Export {
    fn from(global: ExportGlobal) -> Export {
        Export::Global(global)
    }
}
