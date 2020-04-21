//! This module contains the types to manipulate and access Wasm modules.
//!
//! A Wasm module is the artifact of compiling WebAssembly. Wasm modules are not executable
//! until they're instantiated with imports (via [`ImportObject`]).
use crate::{
    backend::RunnableModule,
    cache::{Artifact, Error as CacheError},
    error,
    import::ImportObject,
    structures::{Map, TypedIndex},
    types::{
        ExportType, FuncIndex, FuncSig, GlobalIndex, GlobalInit, GlobalType, ImportType,
        ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex,
        Initializer, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType,
        SigIndex, TableIndex, TableType,
    },
    Instance,
};

use crate::backend::CacheGen;
#[cfg(feature = "generate-debug-information")]
use crate::jit_debug;
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
    pub memories: Map<LocalMemoryIndex, MemoryType>,
    /// Map of global index to global descriptors.
    pub globals: Map<LocalGlobalIndex, GlobalInit>,
    /// Map of table index to table descriptors.
    pub tables: Map<LocalTableIndex, TableType>,

    /// Map of imported function index to import name.
    // These are strictly imported and the typesystem ensures that.
    pub imported_functions: Map<ImportedFuncIndex, ImportName>,
    /// Map of imported memory index to import name and memory descriptor.
    pub imported_memories: Map<ImportedMemoryIndex, (ImportName, MemoryType)>,
    /// Map of imported table index to import name and table descriptor.
    pub imported_tables: Map<ImportedTableIndex, (ImportName, TableType)>,
    /// Map of imported global index to import name and global descriptor.
    pub imported_globals: Map<ImportedGlobalIndex, (ImportName, GlobalType)>,

    /// Map of string to export index.
    // Implementation note: this should maintain the order that the exports appear in the
    // Wasm module.  Be careful not to use APIs that may break the order!
    // Side note, because this is public we can't actually guarantee that it will remain
    // in order.
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
    pub backend: String,

    /// Table of namespace indexes.
    pub namespace_table: StringTable<NamespaceIndex>,
    /// Table of name indexes.
    pub name_table: StringTable<NameIndex>,

    /// Symbol information from emscripten.
    pub em_symbol_map: Option<HashMap<u32, String>>,

    /// Custom sections.
    pub custom_sections: HashMap<String, Vec<Vec<u8>>>,

    /// Flag controlling whether or not debug information for use in a debugger
    /// will be generated.
    pub generate_debug_info: bool,

    #[cfg(feature = "generate-debug-information")]
    #[serde(skip)]
    /// Resource manager of debug information being used by a debugger.
    pub(crate) debug_info_manager: jit_debug::JitCodeDebugInfoManager,
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
                let entry: &mut Vec<Vec<u8>> = self.custom_sections.entry(name).or_default();
                entry.push(data);
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

    /// Get the [`ExportType`]s of the exports this [`Module`] provides.
    pub fn exports(&self) -> Vec<ExportType> {
        self.inner.exports()
    }

    /// Get the [`ImportType`]s describing the imports this [`Module`]
    /// requires to be instantiated.
    pub fn imports(&self) -> Vec<ImportType> {
        let mut out = Vec::with_capacity(
            self.inner.info.imported_functions.len()
                + self.inner.info.imported_memories.len()
                + self.inner.info.imported_tables.len()
                + self.inner.info.imported_globals.len(),
        );

        /// Lookup the (namespace, name) in the [`ModuleInfo`] by index.
        fn get_import_name(
            info: &ModuleInfo,
            &ImportName {
                namespace_index,
                name_index,
            }: &ImportName,
        ) -> (String, String) {
            let namespace = info.namespace_table.get(namespace_index).to_string();
            let name = info.name_table.get(name_index).to_string();

            (namespace, name)
        }

        let info = &self.inner.info;

        let imported_functions = info.imported_functions.iter().map(|(idx, import_name)| {
            let (namespace, name) = get_import_name(info, import_name);
            let sig = info
                .signatures
                .get(*info.func_assoc.get(FuncIndex::new(idx.index())).unwrap())
                .unwrap();
            ImportType {
                namespace,
                name,
                ty: sig.into(),
            }
        });
        let imported_memories =
            info.imported_memories
                .values()
                .map(|(import_name, memory_descriptor)| {
                    let (namespace, name) = get_import_name(info, import_name);
                    ImportType {
                        namespace,
                        name,
                        ty: memory_descriptor.into(),
                    }
                });
        let imported_tables =
            info.imported_tables
                .values()
                .map(|(import_name, table_descriptor)| {
                    let (namespace, name) = get_import_name(info, import_name);
                    ImportType {
                        namespace,
                        name,
                        ty: table_descriptor.into(),
                    }
                });
        let imported_globals =
            info.imported_globals
                .values()
                .map(|(import_name, global_descriptor)| {
                    let (namespace, name) = get_import_name(info, import_name);
                    ImportType {
                        namespace,
                        name,
                        ty: global_descriptor.into(),
                    }
                });

        out.extend(imported_functions);
        out.extend(imported_memories);
        out.extend(imported_tables);
        out.extend(imported_globals);
        out
    }

    /// Get the custom sections matching the given name.
    pub fn custom_sections(&self, key: impl AsRef<str>) -> Option<&[Vec<u8>]> {
        let key = key.as_ref();
        self.inner.info.custom_sections.get(key).map(|v| v.as_ref())
    }
}

impl Clone for Module {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl ModuleInner {
    /// Iterate over the [`ExportType`]s of the exports that this module provides.
    pub(crate) fn exports_iter(&self) -> impl Iterator<Item = ExportType> + '_ {
        self.info.exports.iter().map(move |(name, &ei)| ExportType {
            name,
            ty: match ei {
                ExportIndex::Func(f_idx) => {
                    let sig_idx = self.info.func_assoc[f_idx].into();
                    self.info.signatures[sig_idx].clone().into()
                }
                ExportIndex::Global(g_idx) => {
                    let info = &self.info;
                    let local_global_idx =
                        LocalGlobalIndex::new(g_idx.index() - info.imported_globals.len());
                    info.globals[local_global_idx].desc.into()
                }
                ExportIndex::Memory(m_idx) => {
                    let info = &self.info;
                    let local_memory_idx =
                        LocalMemoryIndex::new(m_idx.index() - info.imported_memories.len());
                    info.memories[local_memory_idx].into()
                }
                ExportIndex::Table(t_idx) => {
                    let info = &self.info;
                    let local_table_idx =
                        LocalTableIndex::new(t_idx.index() - info.imported_tables.len());
                    info.tables[local_table_idx].into()
                }
            },
        })
    }

    /// Get the [`ExportType`]s of the exports this [`Module`] provides.
    pub fn exports(&self) -> Vec<ExportType> {
        self.exports_iter().collect()
    }
}

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
/// [`FuncSig`]), [`GlobalInit`]s, [`MemoryType`]s, and
/// [`TableType`]s.
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
