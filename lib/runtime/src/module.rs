use crate::{
    backend::FuncResolver,
    import::ImportResolver,
    sig_registry::SigRegistry,
    types::{
        FuncIndex, Global, GlobalDesc, GlobalIndex, Map, MapIndex, Memory, MemoryIndex, SigIndex,
        Table, TableIndex,
    },
    Instance,
};
use hashbrown::HashMap;
use std::ops::Deref;
use std::rc::Rc;

/// This is used to instantiate a new webassembly module.
pub struct ModuleInner {
    pub func_resolver: Box<dyn FuncResolver>,
    pub memories: Map<MemoryIndex, Memory>,
    pub globals: Map<GlobalIndex, Global>,
    pub tables: Map<TableIndex, Table>,

    pub imported_functions: Map<FuncIndex, ImportName>,
    pub imported_memories: Map<MemoryIndex, (ImportName, Memory)>,
    pub imported_tables: Map<TableIndex, (ImportName, Table)>,
    pub imported_globals: Map<GlobalIndex, (ImportName, GlobalDesc)>,

    pub exports: HashMap<String, ExportIndex>,

    pub data_initializers: Vec<DataInitializer>,
    pub table_initializers: Vec<TableInitializer>,
    pub start_func: Option<FuncIndex>,

    pub func_assoc: Map<FuncIndex, SigIndex>,
    pub sig_registry: SigRegistry,
}

pub struct Module(Rc<ModuleInner>);

impl Module {
    #[inline]
    pub fn new(inner: ModuleInner) -> Self {
        Module(Rc::new(inner))
    }

    /// Instantiate a webassembly module with the provided imports.
    pub fn instantiate(&self, imports: Rc<dyn ImportResolver>) -> Result<Box<Instance>, String> {
        Instance::new(Module(Rc::clone(&self.0)), imports)
    }
}

impl ModuleInner {
    pub(crate) fn is_imported_function(&self, func_index: FuncIndex) -> bool {
        func_index.index() < self.imported_functions.len()
    }

    pub(crate) fn is_imported_memory(&self, memory_index: MemoryIndex) -> bool {
        memory_index.index() < self.imported_memories.len()
    }
}

impl Deref for Module {
    type Target = ModuleInner;

    fn deref(&self) -> &ModuleInner {
        &*self.0
    }
}

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
#[derive(Debug)]
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
#[derive(Clone, Debug)]
pub struct TableInitializer {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: usize,
    /// The values to write into the table elements.
    pub elements: Vec<FuncIndex>,
}
