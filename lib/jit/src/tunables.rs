use wasm_common::{MemoryType, TableType};
use wasmer_runtime::{LinearMemory, Table};
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
}
