use wasmer_vm::{
    ImportInitializerFuncPtr, VMExport, VMExportFunction, VMExportGlobal, VMExportMemory,
    VMExportTable,
};

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

impl From<Export> for VMExport {
    fn from(other: Export) -> Self {
        match other {
            Export::Function(ExportFunction { vm_function, .. }) => VMExport::Function(vm_function),
            Export::Memory(ExportMemory { vm_memory }) => VMExport::Memory(vm_memory),
            Export::Table(ExportTable { vm_table }) => VMExport::Table(vm_table),
            Export::Global(ExportGlobal { vm_global }) => VMExport::Global(vm_global),
        }
    }
}

impl From<VMExport> for Export {
    fn from(other: VMExport) -> Self {
        match other {
            VMExport::Function(vm_function) => Export::Function(ExportFunction {
                vm_function,
                import_init_function_ptr: None,
            }),
            VMExport::Memory(vm_memory) => Export::Memory(ExportMemory { vm_memory }),
            VMExport::Table(vm_table) => Export::Table(ExportTable { vm_table }),
            VMExport::Global(vm_global) => Export::Global(ExportGlobal { vm_global }),
        }
    }
}

/// A function export value with an extra function pointer to initialize
/// host environments.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFunction {
    /// The VM function, containing most of the data.
    pub vm_function: VMExportFunction,
    /// Function pointer to `WasmerEnv::init_with_instance(&mut self, instance: &Instance)`.
    ///
    /// This function is called to finish setting up the environment after
    /// we create the `api::Instance`.
    pub import_init_function_ptr: Option<ImportInitializerFuncPtr>,
}

impl From<ExportFunction> for Export {
    fn from(func: ExportFunction) -> Self {
        Self::Function(func)
    }
}

/// A table export value.
#[derive(Debug, Clone)]
pub struct ExportTable {
    /// The VM table, containing info about the table.
    pub vm_table: VMExportTable,
}

impl From<ExportTable> for Export {
    fn from(table: ExportTable) -> Self {
        Self::Table(table)
    }
}

/// A memory export value.
#[derive(Debug, Clone)]
pub struct ExportMemory {
    /// The VM memory, containing info about the table.
    pub vm_memory: VMExportMemory,
}

impl From<ExportMemory> for Export {
    fn from(memory: ExportMemory) -> Self {
        Self::Memory(memory)
    }
}

/// A global export value.
#[derive(Debug, Clone)]
pub struct ExportGlobal {
    /// The VM global, containing info about the global.
    pub vm_global: VMExportGlobal,
}

impl From<ExportGlobal> for Export {
    fn from(global: ExportGlobal) -> Self {
        Self::Global(global)
    }
}
