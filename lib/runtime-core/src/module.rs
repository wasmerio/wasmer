//! The module module contains the implementation data structures and helper functions used to
//! manipulate and access wasm modules.
use crate::{
    backend::{Backend, RunnableModule},
    cache::{Artifact, Error as CacheError},
    error,
    import::ImportObject,
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalDescriptor, GlobalIndex, GlobalInit, ImportedFuncIndex,
        ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex, Initializer,
        LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryDescriptor, MemoryIndex,
        SigIndex, TableDescriptor, TableIndex,
    },
    Instance,
};

use crate::backend::CacheGen;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

/// This is used to instantiate a new WebAssembly module.
#[doc(hidden)]
pub struct ModuleInner {
    pub runnable_module: Arc<Box<dyn RunnableModule>>,
    pub cache_gen: Box<dyn CacheGen>,
    pub info: ModuleInfo,
}

/// Container for module data including memories, globals, tables, imports, and exports.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Map of memory index to memory descriptors.
    // This are strictly local and the typesystem ensures that.
    pub memories: Map<LocalMemoryIndex, MemoryDescriptor>,
    /// Map of global index to global descriptors.
    pub globals: Map<LocalGlobalIndex, GlobalInit>,
    /// Map of table index to table descriptors.
    pub tables: Map<LocalTableIndex, TableDescriptor>,

    /// Map of imported function index to import name.
    // These are strictly imported and the typesystem ensures that.
    pub imported_functions: Map<ImportedFuncIndex, ImportName>,
    /// Map of imported memory index to import name and memory descriptor.
    pub imported_memories: Map<ImportedMemoryIndex, (ImportName, MemoryDescriptor)>,
    /// Map of imported table index to import name and table descriptor.
    pub imported_tables: Map<ImportedTableIndex, (ImportName, TableDescriptor)>,
    /// Map of imported global index to import name and global descriptor.
    pub imported_globals: Map<ImportedGlobalIndex, (ImportName, GlobalDescriptor)>,

    /// Map of string to export index.
    pub exports: IndexMap<String, ExportIndex>,

    /// Vector of data initializers.
    pub data_initializers: Vec<DataInitializer>,
    /// Vector of table initializers.
    pub elem_initializers: Vec<TableInitializer>,

    /// Index of optional start function.
    pub start_func: Option<FuncIndex>,

    /// Map function index to signature index.
    pub func_assoc: Map<FuncIndex, SigIndex>,
    /// Map signature index to function signature.
    pub signatures: Map<SigIndex, FuncSig>,
    /// Backend.
    pub backend: Backend,

    /// Table of namespace indexes.
    pub namespace_table: StringTable<NamespaceIndex>,
    /// Table of name indexes.
    pub name_table: StringTable<NameIndex>,

    /// Symbol information from emscripten.
    pub em_symbol_map: Option<HashMap<u32, String>>,

    /// Custom sections.
    pub custom_sections: HashMap<String, Vec<u8>>,
}

impl ModuleInfo {
    /// Creates custom section info from the given wasm file.
    pub fn import_custom_sections(&mut self, wasm: &[u8]) -> crate::error::ParseResult<()> {
        let mut parser = wasmparser::ModuleReader::new(wasm)?;
        while !parser.eof() {
            let section = parser.read()?;
            if let wasmparser::SectionCode::Custom { name, kind: _ } = section.code {
                let mut reader = section.get_binary_reader();
                let len = reader.bytes_remaining();
                let bytes = reader.read_bytes(len)?;
                let data = bytes.to_vec();
                let name = name.to_string();
                self.custom_sections.insert(name, data);
            }
        }
        Ok(())
    }
}

/// A compiled WebAssembly module.
///
/// `Module` is returned by the [`compile_with`][] function.
///
/// [`compile_with`]: crate::compile_with
pub struct Module {
    inner: Arc<ModuleInner>,
}

impl Module {
    pub(crate) fn new(inner: Arc<ModuleInner>) -> Self {
        Module { inner }
    }

    /// Instantiate a WebAssembly module with the provided [`ImportObject`].
    ///
    /// [`ImportObject`]: struct.ImportObject.html
    ///
    /// # Note:
    /// Instantiating a `Module` will also call the function designated as `start`
    /// in the WebAssembly module, if there is one.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::Module;
    /// # use wasmer_runtime_core::imports;
    /// # fn instantiate(module: &Module) -> Result<()> {
    /// let import_object = imports! {
    ///     // ...
    /// };
    /// let instance = module.instantiate(&import_object)?;
    /// // ...
    /// # Ok(())
    /// # }
    /// ```
    pub fn instantiate(&self, import_object: &ImportObject) -> error::Result<Instance> {
        Instance::new(Arc::clone(&self.inner), import_object)
    }

    /// Create a cache artifact from this module.
    pub fn cache(&self) -> Result<Artifact, CacheError> {
        let (backend_metadata, code) = self.inner.cache_gen.generate_cache()?;
        Ok(Artifact::from_parts(
            Box::new(self.inner.info.clone()),
            backend_metadata,
            code,
        ))
    }

    /// Get the module data for this module.
    pub fn info(&self) -> &ModuleInfo {
        &self.inner.info
    }
}

impl Clone for Module {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ModuleInner {}

#[doc(hidden)]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImportName {
    pub namespace_index: NamespaceIndex,
    pub name_index: NameIndex,
}

/// A wrapper around the [`TypedIndex`]es for Wasm functions, Wasm memories,
/// Wasm globals, and Wasm tables.
///
/// Used in [`ModuleInfo`] to access function signatures ([`SigIndex`]s,
/// [`FuncSig`]), [`GlobalInit`]s, [`MemoryDescriptor`]s, and
/// [`TableDescriptor`]s.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportIndex {
    /// Function export index. [`FuncIndex`] is a type-safe handle referring to
    /// a Wasm function.
    Func(FuncIndex),
    /// Memory export index. [`MemoryIndex`] is a type-safe handle referring to
    /// a Wasm memory.
    Memory(MemoryIndex),
    /// Global export index. [`GlobalIndex`] is a type-safe handle referring to
    /// a Wasm global.
    Global(GlobalIndex),
    /// Table export index. [`TableIndex`] is a type-safe handle referring to
    /// to a Wasm table.
    Table(TableIndex),
}

/// A data initializer for linear memory.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DataInitializer {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The initialization data.
    #[cfg_attr(feature = "cache", serde(with = "serde_bytes"))]
    pub data: Vec<u8>,
}

/// A WebAssembly table initializer.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TableInitializer {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The values to write into the table elements.
    pub elements: Vec<FuncIndex>,
}

/// String table builder.
pub struct StringTableBuilder<K: TypedIndex> {
    map: IndexMap<String, (K, u32, u32)>,
    buffer: String,
    count: u32,
}

impl<K: TypedIndex> StringTableBuilder<K> {
    /// Creates a new [`StringTableBuilder`].
    pub fn new() -> Self {
        Self {
            map: IndexMap::new(),
            buffer: String::new(),
            count: 0,
        }
    }

    /// Register a new string into table.
    pub fn register<S>(&mut self, s: S) -> K
    where
        S: Into<String> + AsRef<str>,
    {
        let s_str = s.as_ref();

        if self.map.contains_key(s_str) {
            self.map[s_str].0
        } else {
            let offset = self.buffer.len();
            let length = s_str.len();
            let index = TypedIndex::new(self.count as _);

            self.buffer.push_str(s_str);
            self.map
                .insert(s.into(), (index, offset as u32, length as u32));
            self.count += 1;

            index
        }
    }

    /// Finish building the [`StringTable`].
    pub fn finish(self) -> StringTable<K> {
        let table = self
            .map
            .values()
            .map(|(_, offset, length)| (*offset, *length))
            .collect();

        StringTable {
            table,
            buffer: self.buffer,
        }
    }
}

/// A map of index to string.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StringTable<K: TypedIndex> {
    table: Map<K, (u32, u32)>,
    buffer: String,
}

impl<K: TypedIndex> StringTable<K> {
    /// Creates a `StringTable`.
    pub fn new() -> Self {
        Self {
            table: Map::new(),
            buffer: String::new(),
        }
    }

    /// Gets a reference to a string at the given index.
    pub fn get(&self, index: K) -> &str {
        let (offset, length) = self.table[index];
        let offset = offset as usize;
        let length = length as usize;

        &self.buffer[offset..offset + length]
    }
}

/// A type-safe handle referring to a module namespace.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NamespaceIndex(u32);

impl TypedIndex for NamespaceIndex {
    #[doc(hidden)]
    fn new(index: usize) -> Self {
        NamespaceIndex(index as _)
    }

    #[doc(hidden)]
    fn index(&self) -> usize {
        self.0 as usize
    }
}

/// A type-safe handle referring to a name in a module namespace.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NameIndex(u32);

impl TypedIndex for NameIndex {
    #[doc(hidden)]
    fn new(index: usize) -> Self {
        NameIndex(index as _)
    }

    #[doc(hidden)]
    fn index(&self) -> usize {
        self.0 as usize
    }
}
