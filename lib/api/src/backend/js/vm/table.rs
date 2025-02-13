use js_sys::WebAssembly::Table as JsTable;
use wasmer_types::TableType;

/// The VM Table type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMTable {
    pub(crate) table: JsTable,
    pub(crate) ty: TableType,
}

unsafe impl Send for VMTable {}
unsafe impl Sync for VMTable {}

impl VMTable {
    pub(crate) fn new(table: JsTable, ty: TableType) -> Self {
        Self { table, ty }
    }

    /// Get the table size at runtime
    pub fn get_runtime_size(&self) -> u32 {
        self.table.length()
    }
}
