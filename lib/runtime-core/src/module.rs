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
        ElementType, FuncIndex, FuncSig, GlobalDescriptor, GlobalIndex, GlobalInit,
        ImportedFuncIndex, ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex,
        Initializer, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryDescriptor,
        MemoryIndex, SigIndex, TableDescriptor, TableIndex, Type,
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

    /// Iterate over the exports that this module provides.
    ///
    /// ```
    /// # use wasmer_runtime_core::module::*;
    /// # fn example(module: &Module) {
    /// // We can filter by `ExportKind` to get only certain types of exports.
    /// // For example, here we get all the names of the functions exported by this module.
    /// let function_names =
    ///     module.exports()
    ///           .filter(|ed| ed.kind == ExportKind::Function)
    ///           .map(|ed| ed.name)
    ///           .collect::<Vec<String>>();
    ///
    /// // And here we count the number of global variables exported by this module.
    /// let num_globals =
    ///     module.exports()
    ///           .filter(|ed| ed.kind == ExportKind::Global)
    ///           .count();
    /// # }
    /// ```
    pub fn exports(&self) -> impl Iterator<Item = ExportDescriptor> + '_ {
        self.inner
            .info
            .exports
            .iter()
            .map(|(name, &ei)| ExportDescriptor {
                name: name.clone(),
                kind: ei.into(),
            })
    }

    /// Get the [`Import`]s this [`Module`] requires to be instantiated.
    pub fn imports(&self) -> Vec<Import> {
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

        let imported_functions = info.imported_functions.values().map(|import_name| {
            let (namespace, name) = get_import_name(info, import_name);
            Import {
                namespace,
                name,
                ty: ImportType::Function,
            }
        });
        let imported_memories =
            info.imported_memories
                .values()
                .map(|(import_name, memory_descriptor)| {
                    let (namespace, name) = get_import_name(info, import_name);
                    Import {
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
                    Import {
                        namespace,
                        name,
                        ty: table_descriptor.into(),
                    }
                });
        let imported_globals =
            info.imported_tables
                .values()
                .map(|(import_name, global_descriptor)| {
                    let (namespace, name) = get_import_name(info, import_name);
                    Import {
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

// TODO: review this vs `ExportIndex`
/// Type describing an export that the [`Module`] provides.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExportDescriptor {
    /// The name identifying the export.
    pub name: String,
    /// The type of the export.
    pub kind: ExportKind,
}

// TODO: kind vs type
/// Tag indicating the type of the export.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExportKind {
    /// The export is a function.
    Function,
    /// The export is a global variable.
    Global,
    /// The export is a linear memory.
    Memory,
    /// The export is a table.
    Table,
}

impl From<ExportIndex> for ExportKind {
    fn from(other: ExportIndex) -> Self {
        match other {
            ExportIndex::Func(_) => Self::Function,
            ExportIndex::Global(_) => Self::Global,
            ExportIndex::Memory(_) => Self::Memory,
            ExportIndex::Table(_) => Self::Table,
        }
    }
}

impl Clone for Module {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// The type of import. This indicates whether the import is a function, global, memory, or table.
// TODO: discuss and research Kind vs Type;
// `Kind` has meaning to me from Haskell as an incomplete type like
// `List` which is of kind `* -> *`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportType {
    /// The import is a function.
    // TODO: why does function have no data?
    Function,
    /// The import is a global variable.
    Global {
        /// Whether or not the variable can be mutated.
        mutable: bool,
        /// The Wasm type that the global variable holds.
        // TODO: attempt to understand explanation about 128bit globals:
        // https://github.com/WebAssembly/simd/blob/master/proposals/simd/SIMD.md#webassembly-module-instatiation
        ty: Type,
    },
    /// A Wasm linear memory.
    // TODO: discuss using `Pages` here vs u32
    Memory {
        /// The minimum number of pages this memory must have.
        minimum_pages: u32,
        /// The maximum number of pages this memory can have.
        maximum_pages: Option<u32>,
        // TODO: missing fields, `shared`, `memory_type`
    },
    /// A Wasm table.
    Table {
        /// The minimum number of elements this table must have.
        minimum_elements: u32,
        /// The maximum number of elements this table can have.
        maximum_elements: Option<u32>,
        /// The type that this table contains
        element_type: ElementType,
    },
}

impl From<MemoryDescriptor> for ImportType {
    fn from(other: MemoryDescriptor) -> Self {
        ImportType::Memory {
            minimum_pages: other.minimum.0,
            maximum_pages: other.maximum.map(|inner| inner.0),
        }
    }
}
impl From<&MemoryDescriptor> for ImportType {
    fn from(other: &MemoryDescriptor) -> Self {
        ImportType::Memory {
            minimum_pages: other.minimum.0,
            maximum_pages: other.maximum.map(|inner| inner.0),
        }
    }
}

impl From<TableDescriptor> for ImportType {
    fn from(other: TableDescriptor) -> Self {
        ImportType::Table {
            minimum_elements: other.minimum,
            maximum_elements: other.maximum,
            element_type: other.element,
        }
    }
}
impl From<&TableDescriptor> for ImportType {
    fn from(other: &TableDescriptor) -> Self {
        ImportType::Table {
            minimum_elements: other.minimum,
            maximum_elements: other.maximum,
            element_type: other.element,
        }
    }
}
impl From<GlobalDescriptor> for ImportType {
    fn from(other: GlobalDescriptor) -> Self {
        ImportType::Global {
            mutable: other.mutable,
            ty: other.ty,
        }
    }
}
impl From<&GlobalDescriptor> for ImportType {
    fn from(other: &GlobalDescriptor) -> Self {
        ImportType::Global {
            mutable: other.mutable,
            ty: other.ty,
        }
    }
}

/// A type describing an import that a [`Module`] needs to be instantiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The namespace that this import is in.
    pub namespace: String,
    /// The name of the import.
    pub name: String,
    /// The type of the import.
    pub ty: ImportType,
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
