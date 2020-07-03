use crate::error::LinkError;
use std::sync::Arc;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    GlobalInit, GlobalType, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex, MemoryIndex,
    MemoryType, Mutability, TableIndex, TableType,
};
use wasmer_runtime::MemoryError;
use wasmer_runtime::{Global, Imports, Memory, ModuleInfo, Table};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// Tunables for an engine
pub trait Tunables {
    /// Construct a `MemoryPlan` for the provided `MemoryType`
    fn memory_plan(&self, memory: MemoryType) -> MemoryPlan;

    /// Construct a `TablePlan` for the provided `TableType`
    fn table_plan(&self, table: TableType) -> TablePlan;

    /// Create a memory given a memory type
    fn create_memory(&self, memory_type: MemoryPlan) -> Result<Arc<dyn Memory>, MemoryError>;

    /// Create a memory given a memory type
    fn create_table(&self, table_type: TablePlan) -> Result<Arc<dyn Table>, String>;

    /// Create a global with the given value.
    fn create_initialized_global(
        &self,
        module: &ModuleInfo,
        imports: &Imports,
        mutability: Mutability,
        init: GlobalInit,
    ) -> Result<Arc<Global>, String> {
        Ok(Global::new_with_init(module, imports, mutability, init).map_err(|e| e.to_string())?)
    }

    /// Create a global with a default value.
    fn create_global(&self, ty: GlobalType) -> Result<Arc<Global>, String> {
        Ok(Arc::new(Global::new(ty)))
    }

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
        imports: &Imports,
    ) -> Result<PrimaryMap<LocalGlobalIndex, Arc<Global>>, LinkError> {
        let num_imports = module.num_imported_globals;
        let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

        for (idx, &global_type) in module.globals.iter().skip(num_imports) {
            let idx = module.local_global_index(idx).unwrap();

            vmctx_globals.push(
                if let Some(&initializer) = module.global_initializers.get(idx) {
                    self.create_initialized_global(
                        module,
                        imports,
                        global_type.mutability,
                        initializer,
                    )
                    .map_err(LinkError::Resource)?
                } else {
                    self.create_global(global_type)
                        .map_err(LinkError::Resource)?
                },
            );
        }

        Ok(vmctx_globals)
    }
}
