use std::sync::{Arc, Mutex};

use js_sys::WebAssembly::Table as JsTable;
use wasmer_types::{TableType, FunctionType};

/// The VM Table type
#[derive(Clone, Debug)]
pub struct VMTable {
    pub(crate) table: JsTable,
    pub(crate) ty: TableType,

    pub(super) func_types: Arc<Mutex<Vec<Option<FunctionType>>>>,
}

impl std::cmp::PartialEq for VMTable {
    fn eq(&self, other: &Self) -> bool {
        self.table == other.table && self.ty == other.ty
    }
}

impl std::cmp::Eq for VMTable {}

unsafe impl Send for VMTable {}
unsafe impl Sync for VMTable {}

impl VMTable {
    pub(crate) fn new(table: JsTable, ty: TableType) -> Self {
        Self {
            table,
            ty,
            func_types: Arc::new(Mutex::new(vec![None; ty.minimum as usize])),
        }
    }

    pub(crate) fn set_func_type(&self, index: u32, func_type: Option<FunctionType>) {
        let mut func_types = self.func_types.lock().unwrap();
        let index = index as usize;
        if index < func_types.len() {
            func_types[index] = func_type;
        } else {
            func_types.push(func_type);
        }
    }

    pub(crate) fn get_func_type(&self, index: u32) -> Option<FunctionType> {
        let func_types = self.func_types.lock().unwrap();
        func_types.get(index as usize).cloned().unwrap_or(None)
    }

    /// Get the table size at runtime
    pub fn get_runtime_size(&self) -> u32 {
        self.table.length()
    }
}
