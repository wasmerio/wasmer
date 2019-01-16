use crate::{
    backend::FuncResolver,
    import::Imports,
    sig_registry::SigRegistry,
    structures::Map,
    types::{
        FuncIndex, Global, GlobalIndex, ImportedFuncIndex, ImportedGlobal, ImportedGlobalIndex,
        ImportedMemoryIndex, ImportedTableIndex, Initializer, LocalGlobalIndex, LocalMemoryIndex,
        LocalTableIndex, Memory, MemoryIndex, SigIndex, Table, TableIndex,
    },
    Instance,
};
use hashbrown::HashMap;
use std::rc::Rc;

/// This is used to instantiate a new webassembly module.
#[doc(hidden)]
pub struct ModuleInner {
    pub func_resolver: Box<dyn FuncResolver>,
    // This are strictly local and the typsystem ensures that.
    pub memories: Map<LocalMemoryIndex, Memory>,
    pub globals: Map<LocalGlobalIndex, Global>,
    pub tables: Map<LocalTableIndex, Table>,

    // These are strictly imported and the typesystem ensures that.
    pub imported_functions: Map<ImportedFuncIndex, ImportName>,
    pub imported_memories: Map<ImportedMemoryIndex, (ImportName, Memory)>,
    pub imported_tables: Map<ImportedTableIndex, (ImportName, Table)>,
    pub imported_globals: Map<ImportedGlobalIndex, (ImportName, ImportedGlobal)>,

    pub exports: HashMap<String, ExportIndex>,

    pub data_initializers: Vec<DataInitializer>,
    pub table_initializers: Vec<TableInitializer>,
    pub start_func: Option<FuncIndex>,

    pub func_assoc: Map<FuncIndex, SigIndex>,
    pub sig_registry: SigRegistry,
}

pub struct Module(Rc<ModuleInner>);

impl Module {
    pub(crate) fn new(inner: Rc<ModuleInner>) -> Self {
        Module(inner)
    }

    /// Instantiate a webassembly module with the provided imports.
    pub fn instantiate(&self, imports: &mut Imports) -> Result<Instance, String> {
        Instance::new(Rc::clone(&self.0), imports)
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
    /// Optionally a globalvalue base to initialize at.
    pub base: Option<GlobalIndex>,
    /// A constant offset to initialize at.
    pub offset: usize,
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
