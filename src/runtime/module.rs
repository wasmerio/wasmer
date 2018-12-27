use crate::runtime::backend::FuncResolver;
use crate::runtime::types::{
    FuncIndex, FuncSig, Global, GlobalDesc, GlobalIndex, Map, Memory, MemoryIndex, SigIndex, Table,
    TableIndex,
};
use hashbrown::HashMap;

/// This is used to instantiate a new webassembly module.
pub struct Module {
    pub functions: Box<dyn FuncResolver>,
    pub memories: Map<Memory, MemoryIndex>,
    pub globals: Map<Global, GlobalIndex>,
    pub tables: Map<Table, TableIndex>,

    pub imported_functions: Map<ImportName, FuncIndex>,
    pub imported_memories: Map<(ImportName, Memory), MemoryIndex>,
    pub imported_tables: Map<(ImportName, Table), TableIndex>,
    pub imported_globals: Map<(ImportName, GlobalDesc), GlobalIndex>,

    pub exports: HashMap<String, Export>,

    pub data_initializers: Vec<DataInitializer>,
    pub start_func: FuncIndex,

    pub signature_assoc: Map<SigIndex, FuncIndex>,
    pub signatures: Map<FuncSig, SigIndex>,
}

#[derive(Debug, Clone)]
pub struct ImportName {
    pub module: String,
    pub name: String,
}

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
