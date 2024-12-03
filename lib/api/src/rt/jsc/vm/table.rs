use rusty_jsc::JSObject;
use wasmer_types::TableType;

/// The VM Table type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMTable {
    pub(crate) table: JSObject,
    pub(crate) ty: TableType,
}

unsafe impl Send for VMTable {}
unsafe impl Sync for VMTable {}

impl VMTable {
    pub(crate) fn new(table: JSObject, ty: TableType) -> Self {
        Self { table, ty }
    }

    /// Get the table size at runtime
    pub fn get_runtime_size(&self) -> u32 {
        unimplemented!();
        // self.table.length()
    }
}
