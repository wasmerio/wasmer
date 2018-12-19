use super::vm::{CCAnyfunc, LocalTable};
use cranelift_wasm::{
    Table as ClifTable,
    TableElementType,
};

pub struct TableScheme {
    table: ClifTable,
    info: TableInfo,
}

impl TableScheme {
    pub fn from_table(table: ClifTable) -> Self {
        Self {
            table,
            info: match table.ty {
                TableElementType::Func => TableInfo::CallerChecks,
                TableElementType::Val(_) => unimplemented!(),
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TableInfo {
    CallerChecks,
}

#[derive(Debug)]
pub struct TableBacking {
    pub elements: Box<[CCAnyfunc]>,
    pub max: Option<u32>,
}

impl TableBacking {
    pub fn new(scheme: &TableScheme) -> Self {
        match (scheme.table.ty, scheme.info) {
            (TableElementType::Func, TableInfo::CallerChecks) => {
                TableBacking {
                    elements: vec![CCAnyfunc::null(); scheme.table.minimum as usize].into(),
                    max: scheme.table.maximum,
                }
            },
            (TableElementType::Val(_), _) => unimplemented!(),
        }
    }

    pub fn into_vm_table(&mut self) -> LocalTable {
        LocalTable {
            base: self.elements.as_mut_ptr() as *mut u8,
            current_elements: self.elements.len(),
        }
    }
}