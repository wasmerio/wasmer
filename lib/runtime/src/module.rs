use crate::{
    backend::{FuncResolver, ProtectedCaller},
    error::Result,
    import::ImportObject,
    sig_registry::SigRegistry,
    structures::Map,
    types::{
        FuncIndex, Global, GlobalDesc, GlobalIndex, ImportedFuncIndex, ImportedGlobalIndex,
        ImportedMemoryIndex, ImportedTableIndex, Initializer, LocalGlobalIndex, LocalMemoryIndex,
        LocalTableIndex, Memory, MemoryIndex, SigIndex, Table, TableIndex,
    },
    Instance,
};
use hashbrown::HashMap;
use std::rc::Rc;

/// This is used to instantiate a new WebAssembly module.
#[doc(hidden)]
pub struct ModuleInner {
    pub func_resolver: Box<dyn FuncResolver>,
    pub protected_caller: Box<dyn ProtectedCaller>,

    // This are strictly local and the typsystem ensures that.
    pub memories: Map<LocalMemoryIndex, Memory>,
    pub globals: Map<LocalGlobalIndex, Global>,
    pub tables: Map<LocalTableIndex, Table>,

    // These are strictly imported and the typesystem ensures that.
    pub imported_functions: Map<ImportedFuncIndex, ImportName>,
    pub imported_memories: Map<ImportedMemoryIndex, (ImportName, Memory)>,
    pub imported_tables: Map<ImportedTableIndex, (ImportName, Table)>,
    pub imported_globals: Map<ImportedGlobalIndex, (ImportName, GlobalDesc)>,

    pub exports: HashMap<String, ExportIndex>,

    pub data_initializers: Vec<DataInitializer>,
    pub elem_initializers: Vec<TableInitializer>,

    pub start_func: Option<FuncIndex>,

    pub func_assoc: Map<FuncIndex, SigIndex>,
    pub sig_registry: SigRegistry,
}

pub struct Module(pub Rc<ModuleInner>);

impl Module {
    pub(crate) fn new(inner: Rc<ModuleInner>) -> Self {
        Module(inner)
    }

    /// Instantiate a WebAssembly module with the provided imports.
    pub fn instantiate(&self, imports: ImportObject) -> Result<Instance> {
        Instance::new(Rc::clone(&self.0), Box::new(imports))
    }
}

impl ModuleInner {}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct ImportName {
    pub namespace: String,
    pub name: String,
}

impl From<(String, String)> for ImportName {
    fn from(n: (String, String)) -> Self {
        ImportName {
            namespace: n.0,
            name: n.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportIndex {
    Func(FuncIndex),
    Memory(MemoryIndex),
    Global(GlobalIndex),
    Table(TableIndex),
}

/// A data initializer for linear memory.
#[derive(Debug, Clone)]
pub struct DataInitializer {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The initialization data.
    pub data: Vec<u8>,
}

/// A WebAssembly table initializer.
#[derive(Debug, Clone)]
pub struct TableInitializer {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The values to write into the table elements.
    pub elements: Vec<FuncIndex>,
}
