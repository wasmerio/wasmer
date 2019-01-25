use super::vm;
use crate::types::{ElementType, TableDesc};

#[derive(Debug, Clone)]
pub enum TableElements {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Vec<vm::Anyfunc>),
}

#[derive(Debug)]
pub struct TableBacking {
    pub elements: TableElements,
    pub max: Option<u32>,
}

impl TableBacking {
    pub fn new(table: TableDesc) -> Self {
        match table.ty {
            ElementType::Anyfunc => {
                let initial_table_backing_len = match table.max {
                    Some(max) => max,
                    None => table.min,
                } as usize;

                Self {
                    elements: TableElements::Anyfunc(vec![
                        vm::Anyfunc::null();
                        initial_table_backing_len
                    ]),
                    max: table.max,
                }
            }
        }
    }

    pub fn into_vm_table(&mut self) -> vm::LocalTable {
        match self.elements {
            TableElements::Anyfunc(ref mut funcs) => vm::LocalTable {
                base: funcs.as_mut_ptr() as *mut u8,
                current_elements: funcs.len(),
                capacity: funcs.capacity(),
            },
        }
    }
}
