use crate::runtime::{
    types::{
        FuncIndex, FuncSig, Global, GlobalDesc, GlobalIndex, Map, MapIndex, Memory, MemoryIndex,
        SigIndex, Table, TableIndex,
    },
    vm,
};
use hashbrown::HashMap;
use std::ptr::NonNull;

/// This is used to instantiate a new webassembly module.
pub struct Module {
    pub function_resolver: Box<dyn Fn(&Module, FuncIndex) -> Option<NonNull<vm::Func>>>,
    pub memories: Map<Memory, MemoryIndex>,
    pub globals: Map<Global, GlobalIndex>,
    pub tables: Map<Table, TableIndex>,

    pub imported_functions: Map<ImportName, FuncIndex>,
    pub imported_memories: Map<(ImportName, Memory), MemoryIndex>,
    pub imported_tables: Map<(ImportName, Table), TableIndex>,
    pub imported_globals: Map<(ImportName, GlobalDesc), GlobalIndex>,

    pub exports: HashMap<String, Export>,

    pub data_initializers: Vec<DataInitializer>,
    pub table_initializers: Vec<TableInitializer>,
    pub start_func: Option<FuncIndex>,

    pub signature_assoc: Map<SigIndex, FuncIndex>,
    pub signatures: Map<FuncSig, SigIndex>,
}

impl Module {
    pub(in crate::runtime) fn is_imported_function(&self, func_index: FuncIndex) -> bool {
        func_index.index() < self.imported_functions.len()
    }
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
