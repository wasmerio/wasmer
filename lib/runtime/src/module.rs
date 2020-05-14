//! Data structure for representing WebAssembly modules
//! in a [`Module`].

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::iter::ExactSizeIterator;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    DataIndex, ElemIndex, ExportIndex, ExportType, ExternType, FunctionIndex, FunctionType,
    GlobalIndex, GlobalInit, GlobalType, ImportIndex, ImportType, LocalFunctionIndex,
    LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType, Pages,
    SignatureIndex, TableIndex, TableType,
};

/// A WebAssembly table initializer.
#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
pub struct TableElements {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: usize,
    /// The values to write into the table elements.
    pub elements: Box<[FunctionIndex]>,
}

/// Implemenation styles for WebAssembly linear memory.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum MemoryStyle {
    /// The actual memory can be resized and moved.
    Dynamic,
    /// Address space is allocated up front.
    Static {
        /// The number of mapped and unmapped pages.
        bound: Pages,
    },
}

/// A WebAssembly linear memory description along with our chosen style for
/// implementing it.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct MemoryPlan {
    /// The WebAssembly linear memory description.
    pub memory: MemoryType,
    /// Our chosen implementation style.
    pub style: MemoryStyle,
    /// Our chosen offset-guard size.
    pub offset_guard_size: u64,
}

/// Implemenation styles for WebAssembly tables.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum TableStyle {
    /// Signatures are stored in the table and checked in the caller.
    CallerChecksSignature,
}

/// A WebAssembly table description along with our chosen style for
/// implementing it.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct TablePlan {
    /// The WebAssembly table description.
    pub table: TableType,
    /// Our chosen implementation style.
    pub style: TableStyle,
}

#[derive(Debug)]
pub struct ModuleId {
    id: usize,
}

impl ModuleId {
    pub fn id(&self) -> String {
        format!("{}", &self.id)
    }
}

impl Default for ModuleId {
    fn default() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, SeqCst),
        }
    }
}

/// A translated WebAssembly module, excluding the function bodies and
/// memory initializers.
#[derive(Debug, Serialize, Deserialize)]
pub struct Module {
    /// A unique identifier (within this process) for this module.
    ///
    /// We skip serialization/deserialization of this field, as it
    /// should be computed by the process.
    #[serde(skip_serializing, skip_deserializing)]
    pub id: ModuleId,

    /// The name of this wasm module, often found in the wasm file.
    pub name: Option<String>,

    /// Imported entities with the (module, field, index_of_the_import)
    ///
    /// Keeping the `index_of_the_import` is important, as there can be
    /// two same references to the same import, and we don't want to confuse
    /// them.
    pub imports: IndexMap<(String, String, u32), ImportIndex>,

    /// Exported entities.
    pub exports: IndexMap<String, ExportIndex>,

    /// The module "start" function, if present.
    pub start_func: Option<FunctionIndex>,

    /// WebAssembly table initializers.
    pub table_elements: Vec<TableElements>,

    /// WebAssembly passive elements.
    pub passive_elements: HashMap<ElemIndex, Box<[FunctionIndex]>>,

    /// WebAssembly passive data segments.
    pub passive_data: HashMap<DataIndex, Arc<[u8]>>,

    /// WebAssembly global initializers.
    pub global_initializers: PrimaryMap<LocalGlobalIndex, GlobalInit>,

    /// WebAssembly function names.
    pub func_names: HashMap<FunctionIndex, String>,

    /// WebAssembly function signatures.
    pub signatures: PrimaryMap<SignatureIndex, FunctionType>,

    /// WebAssembly functions (imported and local).
    pub functions: PrimaryMap<FunctionIndex, SignatureIndex>,

    /// WebAssembly tables (imported and local).
    pub tables: PrimaryMap<TableIndex, TableType>,

    /// WebAssembly linear memory plans (imported and local).
    pub memories: PrimaryMap<MemoryIndex, MemoryType>,

    /// WebAssembly global variables (imported and local).
    pub globals: PrimaryMap<GlobalIndex, GlobalType>,

    /// Number of imported functions in the module.
    pub num_imported_funcs: usize,

    /// Number of imported tables in the module.
    pub num_imported_tables: usize,

    /// Number of imported memories in the module.
    pub num_imported_memories: usize,

    /// Number of imported globals in the module.
    pub num_imported_globals: usize,
}

impl Module {
    /// Allocates the module data structures.
    pub fn new() -> Self {
        Self {
            id: ModuleId::default(),
            name: None,
            imports: IndexMap::new(),
            exports: IndexMap::new(),
            start_func: None,
            table_elements: Vec::new(),
            passive_elements: HashMap::new(),
            passive_data: HashMap::new(),
            global_initializers: PrimaryMap::new(),
            func_names: HashMap::new(),
            signatures: PrimaryMap::new(),
            functions: PrimaryMap::new(),
            tables: PrimaryMap::new(),
            memories: PrimaryMap::new(),
            globals: PrimaryMap::new(),
            num_imported_funcs: 0,
            num_imported_tables: 0,
            num_imported_memories: 0,
            num_imported_globals: 0,
        }
    }

    /// Get the given passive element, if it exists.
    pub fn get_passive_element(&self, index: ElemIndex) -> Option<&[FunctionIndex]> {
        self.passive_elements.get(&index).map(|es| &**es)
    }

    /// Get the exported signatures of the module
    pub fn exported_signatures(&self) -> Vec<FunctionType> {
        self.exports
            .iter()
            .filter_map(|(_name, export_index)| match export_index {
                ExportIndex::Function(i) => {
                    let signature = self.functions.get(i.clone()).unwrap();
                    let func_type = self.signatures.get(signature.clone()).unwrap();
                    Some(func_type.clone())
                }
                _ => None,
            })
            .collect::<Vec<FunctionType>>()
    }

    /// Get the export types of the module
    pub fn exports<'a>(&'a self) -> ExportsIterator<impl Iterator<Item = ExportType> + 'a> {
        let iter = self.exports.iter().map(move |(name, export_index)| {
            let extern_type = match export_index {
                ExportIndex::Function(i) => {
                    let signature = self.functions.get(i.clone()).unwrap();
                    let func_type = self.signatures.get(signature.clone()).unwrap();
                    ExternType::Function(func_type.clone())
                }
                ExportIndex::Table(i) => {
                    let table_type = self.tables.get(i.clone()).unwrap();
                    ExternType::Table(*table_type)
                }
                ExportIndex::Memory(i) => {
                    let memory_type = self.memories.get(i.clone()).unwrap();
                    ExternType::Memory(*memory_type)
                }
                ExportIndex::Global(i) => {
                    let global_type = self.globals.get(i.clone()).unwrap();
                    ExternType::Global(global_type.clone())
                }
            };
            ExportType::new(name, extern_type)
        });
        ExportsIterator {
            iter,
            size: self.exports.len(),
        }
    }

    /// Get the export types of the module
    pub fn imports<'a>(&'a self) -> ImportsIterator<impl Iterator<Item = ImportType> + 'a> {
        let iter = self
            .imports
            .iter()
            .map(move |((module, field, _), import_index)| {
                let extern_type = match import_index {
                    ImportIndex::Function(i) => {
                        let signature = self.functions.get(i.clone()).unwrap();
                        let func_type = self.signatures.get(signature.clone()).unwrap();
                        ExternType::Function(func_type.clone())
                    }
                    ImportIndex::Table(i) => {
                        let table_type = self.tables.get(i.clone()).unwrap();
                        ExternType::Table(*table_type)
                    }
                    ImportIndex::Memory(i) => {
                        let memory_type = self.memories.get(i.clone()).unwrap();
                        ExternType::Memory(*memory_type)
                    }
                    ImportIndex::Global(i) => {
                        let global_type = self.globals.get(i.clone()).unwrap();
                        ExternType::Global(*global_type)
                    }
                };
                ImportType::new(module, field, extern_type)
            });
        ImportsIterator {
            iter,
            size: self.imports.len(),
        }
    }

    /// Convert a `LocalFunctionIndex` into a `FunctionIndex`.
    pub fn func_index(&self, local_func: LocalFunctionIndex) -> FunctionIndex {
        FunctionIndex::new(self.num_imported_funcs + local_func.index())
    }

    /// Convert a `FunctionIndex` into a `LocalFunctionIndex`. Returns None if the
    /// index is an imported function.
    pub fn local_func_index(&self, func: FunctionIndex) -> Option<LocalFunctionIndex> {
        func.index()
            .checked_sub(self.num_imported_funcs)
            .map(LocalFunctionIndex::new)
    }

    /// Test whether the given function index is for an imported function.
    pub fn is_imported_function(&self, index: FunctionIndex) -> bool {
        index.index() < self.num_imported_funcs
    }

    /// Convert a `LocalTableIndex` into a `TableIndex`.
    pub fn table_index(&self, local_table: LocalTableIndex) -> TableIndex {
        TableIndex::new(self.num_imported_tables + local_table.index())
    }

    /// Convert a `TableIndex` into a `LocalTableIndex`. Returns None if the
    /// index is an imported table.
    pub fn local_table_index(&self, table: TableIndex) -> Option<LocalTableIndex> {
        table
            .index()
            .checked_sub(self.num_imported_tables)
            .map(LocalTableIndex::new)
    }

    /// Test whether the given table index is for an imported table.
    pub fn is_imported_table(&self, index: TableIndex) -> bool {
        index.index() < self.num_imported_tables
    }

    /// Convert a `LocalMemoryIndex` into a `MemoryIndex`.
    pub fn memory_index(&self, local_memory: LocalMemoryIndex) -> MemoryIndex {
        MemoryIndex::new(self.num_imported_memories + local_memory.index())
    }

    /// Convert a `MemoryIndex` into a `LocalMemoryIndex`. Returns None if the
    /// index is an imported memory.
    pub fn local_memory_index(&self, memory: MemoryIndex) -> Option<LocalMemoryIndex> {
        memory
            .index()
            .checked_sub(self.num_imported_memories)
            .map(LocalMemoryIndex::new)
    }

    /// Test whether the given memory index is for an imported memory.
    pub fn is_imported_memory(&self, index: MemoryIndex) -> bool {
        index.index() < self.num_imported_memories
    }

    /// Convert a `LocalGlobalIndex` into a `GlobalIndex`.
    pub fn global_index(&self, local_global: LocalGlobalIndex) -> GlobalIndex {
        GlobalIndex::new(self.num_imported_globals + local_global.index())
    }

    /// Convert a `GlobalIndex` into a `LocalGlobalIndex`. Returns None if the
    /// index is an imported global.
    pub fn local_global_index(&self, global: GlobalIndex) -> Option<LocalGlobalIndex> {
        global
            .index()
            .checked_sub(self.num_imported_globals)
            .map(LocalGlobalIndex::new)
    }

    /// Test whether the given global index is for an imported global.
    pub fn is_imported_global(&self, index: GlobalIndex) -> bool {
        index.index() < self.num_imported_globals
    }

    /// Get the Module name
    pub fn name(&self) -> String {
        match self.name {
            Some(ref name) => name.to_string(),
            None => "<module>".to_string(),
        }
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// Code inspired from
// https://www.reddit.com/r/rust/comments/9vspv4/extending_iterators_ergonomically/

/// This iterator allows us to iterate over the exports
/// and offer nice API ergonomics over it.
pub struct ExportsIterator<I: Iterator<Item = ExportType> + Sized> {
    iter: I,
    size: usize,
}

impl<I: Iterator<Item = ExportType> + Sized> ExactSizeIterator for ExportsIterator<I> {
    // We can easily calculate the remaining number of iterations.
    fn len(&self) -> usize {
        self.size
    }
}

impl<I: Iterator<Item = ExportType> + Sized> ExportsIterator<I> {
    /// Get only the functions
    pub fn functions(self) -> impl Iterator<Item = ExportType<FunctionType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Function(ty) => Some(ExportType::new(extern_.name(), ty.clone())),
            _ => None,
        })
    }
    /// Get only the memories
    pub fn memories(self) -> impl Iterator<Item = ExportType<MemoryType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Memory(ty) => Some(ExportType::new(extern_.name(), *ty)),
            _ => None,
        })
    }
    /// Get only the tables
    pub fn tables(self) -> impl Iterator<Item = ExportType<TableType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Table(ty) => Some(ExportType::new(extern_.name(), *ty)),
            _ => None,
        })
    }
    /// Get only the globals
    pub fn globals(self) -> impl Iterator<Item = ExportType<GlobalType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Global(ty) => Some(ExportType::new(extern_.name(), *ty)),
            _ => None,
        })
    }
}

impl<I: Iterator<Item = ExportType> + Sized> Iterator for ExportsIterator<I> {
    type Item = ExportType;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// This iterator allows us to iterate over the imports
/// and offer nice API ergonomics over it.
pub struct ImportsIterator<I: Iterator<Item = ImportType> + Sized> {
    iter: I,
    size: usize,
}

impl<I: Iterator<Item = ImportType> + Sized> ExactSizeIterator for ImportsIterator<I> {
    // We can easily calculate the remaining number of iterations.
    fn len(&self) -> usize {
        self.size
    }
}

impl<I: Iterator<Item = ImportType> + Sized> ImportsIterator<I> {
    /// Get only the functions
    pub fn functions(self) -> impl Iterator<Item = ImportType<FunctionType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Function(ty) => Some(ImportType::new(
                extern_.module(),
                extern_.name(),
                ty.clone(),
            )),
            _ => None,
        })
    }
    /// Get only the memories
    pub fn memories(self) -> impl Iterator<Item = ImportType<MemoryType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Memory(ty) => Some(ImportType::new(extern_.module(), extern_.name(), *ty)),
            _ => None,
        })
    }
    /// Get only the tables
    pub fn tables(self) -> impl Iterator<Item = ImportType<TableType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Table(ty) => Some(ImportType::new(extern_.module(), extern_.name(), *ty)),
            _ => None,
        })
    }
    /// Get only the globals
    pub fn globals(self) -> impl Iterator<Item = ImportType<GlobalType>> + Sized {
        self.iter.filter_map(|extern_| match extern_.ty() {
            ExternType::Global(ty) => Some(ImportType::new(extern_.module(), extern_.name(), *ty)),
            _ => None,
        })
    }
}

impl<I: Iterator<Item = ImportType> + Sized> Iterator for ImportsIterator<I> {
    type Item = ImportType;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
