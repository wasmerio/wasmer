use crate::runtime::types::{
    Map,
    FuncIndex, MemoryIndex, TableIndex, GlobalIndex,
    Memory, Globals, GlobalDesc, Func, Table,
};
use crate::runtime::backend::FuncResolver;

/// This is used to instantiate a new webassembly module.
#[derive(Debug)]
pub struct Module {
    pub functions: Box<dyn FuncResolver>,
    pub memories: Map<Memory, MemoryIndex>,
    pub globals: Map<Global, GlobalIndex>,
    pub tables: Map<Table, TableIndex>,
    
    pub imported_functions: Map<(ImportName, Func), FuncIndex>,
    pub imported_memories: Map<(ImportName, Memory), MemoryIndex>,
    pub imported_tables: Map<(ImportName, Table), TableIndex>,
    pub imported_globals: Map<(ImportName, GlobalDesc), GlobalIndex>,

    pub exported: Vec<(ItemName, Export)>,

    pub data_initializers: Vec<DataInitializer>,
    pub start_func: FuncIndex,

    pub signatures: Map<Func, FuncIndex>,
}

pub type ModuleName = Vec<u8>;
pub type ItemName = Vec<u8>;
pub type ImportName = (ModuleName, ItemName);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
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