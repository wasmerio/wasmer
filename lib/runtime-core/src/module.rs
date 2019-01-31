use crate::{
    backend::{Backend, FuncResolver, ProtectedCaller},
    error::Result,
    import::ImportObject,
    structures::Map,
    types::{
        FuncIndex, FuncSig, GlobalDescriptor, GlobalIndex, GlobalInit, ImportedFuncIndex,
        ImportedGlobalIndex, ImportedMemoryIndex, ImportedTableIndex, Initializer,
        LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryDescriptor, MemoryIndex,
        SigIndex, TableDescriptor, TableIndex,
    },
    Instance,
};
use hashbrown::HashMap;
use std::sync::Arc;

/// This is used to instantiate a new WebAssembly module.
#[doc(hidden)]
pub struct ModuleInner {
    pub func_resolver: Box<dyn FuncResolver>,
    pub protected_caller: Box<dyn ProtectedCaller>,

    pub info: ModuleInfo,
}

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
pub struct ModuleInfo {
    // This are strictly local and the typsystem ensures that.
    pub memories: Map<LocalMemoryIndex, MemoryDescriptor>,
    pub globals: Map<LocalGlobalIndex, GlobalInit>,
    pub tables: Map<LocalTableIndex, TableDescriptor>,

    // These are strictly imported and the typesystem ensures that.
    pub imported_functions: Map<ImportedFuncIndex, ImportName>,
    pub imported_memories: Map<ImportedMemoryIndex, (ImportName, MemoryDescriptor)>,
    pub imported_tables: Map<ImportedTableIndex, (ImportName, TableDescriptor)>,
    pub imported_globals: Map<ImportedGlobalIndex, (ImportName, GlobalDescriptor)>,

    pub exports: HashMap<String, ExportIndex>,

    pub data_initializers: Vec<DataInitializer>,
    pub elem_initializers: Vec<TableInitializer>,

    pub start_func: Option<FuncIndex>,

    pub func_assoc: Map<FuncIndex, SigIndex>,
    pub signatures: Map<SigIndex, Arc<FuncSig>>,
    pub backend: Backend,
}

/// A compiled WebAssembly module.
///
/// `Module` is returned by the [`compile`] and
/// [`compile_with`] functions.
///
/// [`compile`]: fn.compile.html
/// [`compile_with`]: fn.compile_with.html
pub struct Module(#[doc(hidden)] pub Arc<ModuleInner>);

impl Module {
    pub(crate) fn new(inner: Arc<ModuleInner>) -> Self {
        Module(inner)
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
    /// let instance = module.instantiate(import_object)?;
    /// // ...
    /// # Ok(())
    /// # }
    /// ```
    pub fn instantiate(&self, import_object: ImportObject) -> Result<Instance> {
        Instance::new(Arc::clone(&self.0), Box::new(import_object))
    }
}

impl ModuleInner {}

#[doc(hidden)]
#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
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

#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportIndex {
    Func(FuncIndex),
    Memory(MemoryIndex),
    Global(GlobalIndex),
    Table(TableIndex),
}

/// A data initializer for linear memory.
#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct DataInitializer {
    /// The index of the memory to initialize.
    pub memory_index: MemoryIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The initialization data.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// A WebAssembly table initializer.
#[cfg_attr(feature = "cache", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct TableInitializer {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Either a constant offset or a `get_global`
    pub base: Initializer,
    /// The values to write into the table elements.
    pub elements: Vec<FuncIndex>,
}
