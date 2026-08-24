use crate::engine::error::LinkError;
use std::ptr::NonNull;
use wasmer_types::{
    FunctionType, GlobalType, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex,
    MemoryType, ModuleInfo, TableIndex, TableType, TagKind, entity::PrimaryMap,
};
use wasmer_vm::{InternalStoreHandle, MemoryError, StoreObjects, VMTag};
use wasmer_vm::{MemoryStyle, TableStyle};
use wasmer_vm::{VMConfig, VMGlobal, VMGlobalDefinition, VMMemory, VMTable};
use wasmer_vm::{VMMemoryDefinition, VMTableDefinition};

/// An engine delegates the creation of memories, tables, and globals
/// to a foreign implementor of this trait.
pub trait Tunables {
    /// Construct a `MemoryStyle` for the provided `MemoryType`
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle;

    /// Construct a `TableStyle` for the provided `TableType`
    fn table_style(&self, table: &TableType) -> TableStyle;

    /// Create a memory owned by the host given a [`MemoryType`] and a [`MemoryStyle`].
    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError>;

    /// Create a memory owned by the VM given a [`MemoryType`] and a [`MemoryStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid location in VM memory.
    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError>;

    /// Create a table owned by the host given a [`TableType`] and a [`TableStyle`].
    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String>;

    /// Create a table owned by the VM given a [`TableType`] and a [`TableStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid location in VM memory.
    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String>;

    /// Create a global with an unset value.
    fn create_global(&self, ty: GlobalType) -> Result<VMGlobal, String> {
        Ok(VMGlobal::new(ty))
    }

    /// Create a global owned by the VM with backing storage in the `VMContext`.
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid location in VM memory.
    unsafe fn create_vm_global(
        &self,
        ty: GlobalType,
        vm_definition_location: NonNull<VMGlobalDefinition>,
    ) -> Result<VMGlobal, String> {
        unsafe { Ok(VMGlobal::new_instance(ty, vm_definition_location)) }
    }

    /// Create a new tag.
    fn create_tag(&self, kind: TagKind, ty: FunctionType) -> Result<VMTag, String> {
        Ok(VMTag::new(kind, ty))
    }

    /// Allocate memory for just the memories of the current module.
    ///
    /// # Safety
    /// - `memory_definition_locations` must point to a valid locations in VM memory.
    #[allow(clippy::result_large_err)]
    unsafe fn create_memories(
        &self,
        context: &mut StoreObjects,
        module: &ModuleInfo,
        memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
        memory_definition_locations: &[NonNull<VMMemoryDefinition>],
    ) -> Result<PrimaryMap<LocalMemoryIndex, InternalStoreHandle<VMMemory>>, LinkError> {
        unsafe {
            let num_imports = module.num_imported_memories;
            let mut memories: PrimaryMap<LocalMemoryIndex, _> =
                PrimaryMap::with_capacity(module.memories.len() - num_imports);
            for ((mi, ty), mdl) in module
                .memories
                .iter()
                .skip(num_imports)
                .zip(memory_definition_locations)
            {
                let style = &memory_styles[mi];
                memories.push(InternalStoreHandle::new(
                    context,
                    self.create_vm_memory(ty, style, *mdl).map_err(|e| {
                        LinkError::Resource(format!("Failed to create memory: {e}"))
                    })?,
                ));
            }
            Ok(memories)
        }
    }

    /// Allocate memory for just the tables of the current module.
    ///
    /// # Safety
    ///
    /// To be done
    #[allow(clippy::result_large_err)]
    unsafe fn create_tables(
        &self,
        context: &mut StoreObjects,
        module: &ModuleInfo,
        table_styles: &PrimaryMap<TableIndex, TableStyle>,
        table_definition_locations: &[NonNull<VMTableDefinition>],
    ) -> Result<PrimaryMap<LocalTableIndex, InternalStoreHandle<VMTable>>, LinkError> {
        unsafe {
            let num_imports = module.num_imported_tables;
            let mut tables: PrimaryMap<LocalTableIndex, _> =
                PrimaryMap::with_capacity(module.tables.len() - num_imports);
            for ((ti, ty), tdl) in module
                .tables
                .iter()
                .skip(num_imports)
                .zip(table_definition_locations)
            {
                let style = &table_styles[ti];
                tables.push(InternalStoreHandle::new(
                    context,
                    self.create_vm_table(ty, style, *tdl)
                        .map_err(LinkError::Resource)?,
                ));
            }
            Ok(tables)
        }
    }

    /// Allocate memory for just the globals of the current module,
    /// with initializers applied.
    #[allow(clippy::result_large_err)]
    fn create_globals(
        &self,
        context: &mut StoreObjects,
        module: &ModuleInfo,
        vm_definition_locations: &[NonNull<VMGlobalDefinition>],
    ) -> Result<PrimaryMap<LocalGlobalIndex, InternalStoreHandle<VMGlobal>>, LinkError> {
        let num_imports = module.num_imported_globals;
        let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

        for (i, &global_type) in module.globals.values().skip(num_imports).enumerate() {
            let location = vm_definition_locations
                .get(i)
                .ok_or_else(|| LinkError::Resource("global definition location missing".into()))?;
            vmctx_globals.push(InternalStoreHandle::new(context, unsafe {
                self.create_vm_global(global_type, *location)
                    .map_err(LinkError::Resource)?
            }));
        }

        Ok(vmctx_globals)
    }

    /// Get the VMConfig for this tunables
    /// Currently, VMConfig have optional Stack size
    /// If wasm_stack_size is left to None (the default value)
    /// then the global stack size will be use
    /// Else the defined stack size will be used. Size is in byte
    /// and the value might be rounded to sane value is needed.
    fn vmconfig(&self) -> &VMConfig {
        &VMConfig {
            wasm_stack_size: None,
        }
    }
}

/// Tunable parameters for WebAssembly compilation.
/// This is the reference implementation of the `Tunables` trait,
/// used by default.
///
/// You can use this as a template for creating a custom Tunables
/// implementation or use composition to wrap your Tunables around
/// this one. The later approach is demonstrated in the
/// tunables-limit-memory example.
#[derive(Clone, Default)]
pub struct BaseTunables {}

impl BaseTunables {
    /// Get the default `BaseTunables`.
    pub fn new() -> Self {
        Self {}
    }
}

impl Tunables for BaseTunables {
    /// Always return Static memory style.
    fn memory_style(&self, _memory: &MemoryType) -> MemoryStyle {
        MemoryStyle::Static
    }

    /// Get a [`TableStyle`] for the provided [`TableType`].
    fn table_style(&self, _table: &TableType) -> TableStyle {
        TableStyle::CallerChecksSignature
    }

    /// Create a memory owned by the host given a [`MemoryType`] and a [`MemoryStyle`].
    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError> {
        VMMemory::new(ty, style)
    }

    /// Create a memory owned by the VM given a [`MemoryType`] and a [`MemoryStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid, owned `VMMemoryDefinition`,
    ///   for example in `VMContext`.
    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError> {
        unsafe { VMMemory::from_definition(ty, style, vm_definition_location) }
    }

    /// Create a table owned by the host given a [`TableType`] and a [`TableStyle`].
    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
        VMTable::new(ty, style)
    }

    /// Create a table owned by the VM given a [`TableType`] and a [`TableStyle`].
    ///
    /// # Safety
    /// - `vm_definition_location` must point to a valid, owned `VMTableDefinition`,
    ///   for example in `VMContext`.
    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String> {
        unsafe { VMTable::from_definition(ty, style, vm_definition_location) }
    }
}

impl Tunables for Box<dyn Tunables + Send + Sync> {
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        self.as_ref().memory_style(memory)
    }

    fn table_style(&self, table: &TableType) -> TableStyle {
        self.as_ref().table_style(table)
    }

    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError> {
        self.as_ref().create_host_memory(ty, style)
    }

    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError> {
        unsafe {
            self.as_ref()
                .create_vm_memory(ty, style, vm_definition_location)
        }
    }

    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
        self.as_ref().create_host_table(ty, style)
    }

    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String> {
        unsafe {
            self.as_ref()
                .create_vm_table(ty, style, vm_definition_location)
        }
    }
}

impl Tunables for std::sync::Arc<dyn Tunables + Send + Sync> {
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        self.as_ref().memory_style(memory)
    }

    fn table_style(&self, table: &TableType) -> TableStyle {
        self.as_ref().table_style(table)
    }

    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError> {
        self.as_ref().create_host_memory(ty, style)
    }

    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError> {
        unsafe {
            self.as_ref()
                .create_vm_memory(ty, style, vm_definition_location)
        }
    }

    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
        self.as_ref().create_host_table(ty, style)
    }

    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String> {
        unsafe {
            self.as_ref()
                .create_vm_table(ty, style, vm_definition_location)
        }
    }
}
