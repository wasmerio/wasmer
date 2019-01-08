use crate::compilers::cranelift::codegen::{converter, CraneliftModule};
use crate::runtime::{
    backend::FuncResolver,
    types::{
        FuncIndex, Global, GlobalDesc, GlobalIndex, Map, MapIndex, Memory, MemoryIndex,
        SigIndex, Table, TableIndex,
    },
    sig_registry::SigRegistry,
};
use crate::runtime::{Import, Imports, Instance};
use std::cell::RefCell;

use hashbrown::HashMap;

/// This is used to instantiate a new webassembly module.
pub struct Module {
    pub func_resolver: Box<dyn FuncResolver>,
    pub memories: Map<MemoryIndex, Memory>,
    pub globals: Map<GlobalIndex, Global>,
    pub tables: Map<TableIndex, Table>,

    pub imported_functions: Map<FuncIndex, ImportName>,
    pub imported_memories: Map<MemoryIndex, (ImportName, Memory)>,
    pub imported_tables: Map<TableIndex, (ImportName, Table)>,
    pub imported_globals: Map<GlobalIndex, (ImportName, GlobalDesc)>,

    pub exports: HashMap<String, Export>,

    pub data_initializers: Vec<DataInitializer>,
    pub table_initializers: Vec<TableInitializer>,
    pub start_func: Option<FuncIndex>,
    pub func_assoc: Map<FuncIndex, SigIndex>,
    pub sig_registry: SigRegistry,

    pub environment: RefCell<Option<Box<dyn ModuleEnvironment>>>,
}

/// Provides hooks relating to setting up an environment for a module's instances
pub trait ModuleEnvironment {
    fn after_instantiate(&self, instance: &mut Instance);
    fn append_imports(&self, import_object: &mut Imports);
}

impl Module {
    pub(in crate::runtime) fn is_imported_function(&self, func_index: FuncIndex) -> bool {
        func_index.index() < self.imported_functions.len()
    }

    pub fn after_instantiate(&self, instance: &mut Instance) {
        if let Some(ref module_environment) = *self.environment.borrow() {
            module_environment.after_instantiate(instance);
        }
    }
}

impl From<CraneliftModule> for Module {
    fn from(cranelift_module: CraneliftModule) -> Self {
        converter::convert_module(cranelift_module)
    }
}

#[derive(Debug, Clone)]
pub struct ImportName {
    pub module: String,
    pub name: String,
}

impl From<(String, String)> for ImportName {
    fn from(n: (String, String)) -> Self {
        ImportName {
            module: n.0,
            name: n.1,
        }
    }
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
