use crate::error::LinkError;
use std::sync::Arc;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType, TableIndex,
    TableType,
};
use wasmer_compiler::Target;
use wasmer_runtime::MemoryError;
use wasmer_runtime::{Memory, ModuleInfo, Table, VMGlobalDefinition};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// Tunables for an engine
pub trait Tunables {
    /// Get the target for this Tunables
    fn target(&self) -> &Target;

    /// Construct a `MemoryPlan` for the provided `MemoryType`
    fn memory_plan(&self, memory: MemoryType) -> MemoryPlan;

    /// Construct a `TablePlan` for the provided `TableType`
    fn table_plan(&self, table: TableType) -> TablePlan;

    /// Create a memory given a memory type
    fn create_memory(&self, memory_type: MemoryPlan) -> Result<Arc<dyn Memory>, MemoryError>;

    /// Create a memory given a memory type
    fn create_table(&self, table_type: TablePlan) -> Result<Arc<dyn Table>, String>;

    /// Allocate memory for just the memories of the current module.
    fn create_memories(
        &self,
        module: &ModuleInfo,
        memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
    ) -> Result<PrimaryMap<LocalMemoryIndex, Arc<dyn Memory>>, LinkError> {
        let num_imports = module.num_imported_memories;
        let mut memories: PrimaryMap<LocalMemoryIndex, _> =
            PrimaryMap::with_capacity(module.memories.len() - num_imports);
        for index in num_imports..module.memories.len() {
            let plan = memory_plans[MemoryIndex::new(index)].clone();
            memories.push(
                self.create_memory(plan)
                    .map_err(|e| LinkError::Resource(format!("Failed to create memory: {}", e)))?,
            );
        }
        Ok(memories)
    }

    /// Allocate memory for just the tables of the current module.
    fn create_tables(
        &self,
        module: &ModuleInfo,
        table_plans: &PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<PrimaryMap<LocalTableIndex, Arc<dyn Table>>, LinkError> {
        let num_imports = module.num_imported_tables;
        let mut tables: PrimaryMap<LocalTableIndex, _> =
            PrimaryMap::with_capacity(module.tables.len() - num_imports);
        for index in num_imports..module.tables.len() {
            let plan = table_plans[TableIndex::new(index)].clone();
            tables.push(self.create_table(plan).map_err(LinkError::Resource)?);
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
