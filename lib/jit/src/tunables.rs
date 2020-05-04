use crate::error::LinkError;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    GlobalIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex, MemoryType,
    TableIndex, TableType,
};
use wasmer_runtime::{LinearMemory, Module, Table, VMGlobalDefinition};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// Tunables for an engine
pub trait Tunables {
    /// Get a `MemoryPlan` for the provided `MemoryType`
    fn memory_plan(&self, memory: MemoryType) -> MemoryPlan;

    /// Get a `TablePlan` for the provided `TableType`
    fn table_plan(&self, table: TableType) -> TablePlan;

    /// Create a memory given a memory type
    fn create_memory(&self, memory_type: MemoryPlan) -> Result<LinearMemory, String>;

    /// Create a memory given a memory type
    fn create_table(&self, table_type: TablePlan) -> Table;

    /// Allocate memory for just the memories of the current module.
    fn create_memories(
        &self,
        module: &Module,
        memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
    ) -> Result<PrimaryMap<LocalMemoryIndex, LinearMemory>, LinkError> {
        let num_imports = module.num_imported_memories;
        let mut memories: PrimaryMap<LocalMemoryIndex, _> =
            PrimaryMap::with_capacity(module.memories.len() - num_imports);
        for index in num_imports..module.memories.len() {
            let plan = memory_plans[MemoryIndex::new(index)].clone();
            memories.push(self.create_memory(plan).map_err(LinkError::Resource)?);
        }
        Ok(memories)
    }

    /// Allocate memory for just the tables of the current module.
    fn create_tables(
        &self,
        module: &Module,
        table_plans: &PrimaryMap<TableIndex, TablePlan>,
    ) -> PrimaryMap<LocalTableIndex, Table> {
        let num_imports = module.num_imported_tables;
        let mut tables: PrimaryMap<LocalTableIndex, _> =
            PrimaryMap::with_capacity(module.tables.len() - num_imports);
        for index in num_imports..module.tables.len() {
            let plan = table_plans[TableIndex::new(index)].clone();
            tables.push(self.create_table(plan));
        }
        tables
    }

    /// Allocate memory for just the globals of the current module,
    /// with initializers applied.
    fn create_globals(&self, module: &Module) -> PrimaryMap<LocalGlobalIndex, VMGlobalDefinition> {
        let num_imports = module.num_imported_globals;
        let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

        for _ in &module.globals.values().as_slice()[num_imports..] {
            vmctx_globals.push(VMGlobalDefinition::new());
        }

        vmctx_globals
    }
}
