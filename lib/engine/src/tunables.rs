use crate::error::LinkError;
use std::sync::Arc;
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{
    GlobalType, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType,
    TableIndex, TableType,
};
use wasmer_vm::MemoryError;
use wasmer_vm::{Global, Memory, ModuleInfo, Table};
use wasmer_vm::{MemoryStyle, TableStyle};

/// An engine delegates the creation of memories, tables, and globals
/// to a foreign implementor of this trait.
pub trait Tunables {
    /// Construct a `MemoryStyle` for the provided `MemoryType`
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle;

    /// Construct a `TableStyle` for the provided `TableType`
    fn table_style(&self, table: &TableType) -> TableStyle;

    /// Create a memory given a memory type
    fn create_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<Arc<dyn Memory>, MemoryError>;

    /// Create a memory given a memory type
    fn create_table(&self, ty: &TableType, style: &TableStyle) -> Result<Arc<dyn Table>, String>;

    /// Create a global with an unset value.
    fn create_global(&self, ty: GlobalType) -> Result<Arc<Global>, String> {
        Ok(Arc::new(Global::new(ty)))
    }

    /// Allocate memory for just the memories of the current module.
    fn create_memories(
        &self,
        module: &ModuleInfo,
        memory_styles: &PrimaryMap<MemoryIndex, MemoryStyle>,
    ) -> Result<PrimaryMap<LocalMemoryIndex, Arc<dyn Memory>>, LinkError> {
        let num_imports = module.num_imported_memories;
        let mut memories: PrimaryMap<LocalMemoryIndex, _> =
            PrimaryMap::with_capacity(module.memories.len() - num_imports);
        for index in num_imports..module.memories.len() {
            let mi = MemoryIndex::new(index);
            let ty = &module.memories[mi];
            let style = &memory_styles[mi];
            memories.push(
                self.create_memory(ty, style)
                    .map_err(|e| LinkError::Resource(format!("Failed to create memory: {}", e)))?,
            );
        }
        Ok(memories)
    }

    /// Allocate memory for just the tables of the current module.
    fn create_tables(
        &self,
        module: &ModuleInfo,
        table_styles: &PrimaryMap<TableIndex, TableStyle>,
    ) -> Result<PrimaryMap<LocalTableIndex, Arc<dyn Table>>, LinkError> {
        let num_imports = module.num_imported_tables;
        let mut tables: PrimaryMap<LocalTableIndex, _> =
            PrimaryMap::with_capacity(module.tables.len() - num_imports);
        for index in num_imports..module.tables.len() {
            let ti = TableIndex::new(index);
            let ty = &module.tables[ti];
            let style = &table_styles[ti];
            tables.push(self.create_table(ty, style).map_err(LinkError::Resource)?);
        }
        Ok(tables)
    }

    /// Allocate memory for just the globals of the current module,
    /// with initializers applied.
    fn create_globals(
        &self,
        module: &ModuleInfo,
    ) -> Result<PrimaryMap<LocalGlobalIndex, Arc<Global>>, LinkError> {
        let num_imports = module.num_imported_globals;
        let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

        for &global_type in module.globals.values().skip(num_imports) {
            vmctx_globals.push(
                self.create_global(global_type)
                    .map_err(LinkError::Resource)?,
            );
        }

        Ok(vmctx_globals)
    }
}
