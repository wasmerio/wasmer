use crate::error::LinkError;
use std::sync::Arc;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType, TableIndex,
    TableType,
};
use wasmer_runtime::MemoryError;
use wasmer_runtime::{Memory, ModuleInfo, Table, VMGlobalDefinition};
use wasmer_runtime::{MemoryStyle, TableStyle};

/// Tunables for an engine
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
    ) -> Result<PrimaryMap<LocalGlobalIndex, VMGlobalDefinition>, LinkError> {
        let num_imports = module.num_imported_globals;
        let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

        for _ in &module.globals.values().as_slice()[num_imports..] {
            vmctx_globals.push(VMGlobalDefinition::new());
        }

        Ok(vmctx_globals)
    }
}
