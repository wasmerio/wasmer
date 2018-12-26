use super::vm;
use crate::runtime::types::{ElementType, Table};

#[derive(Debug, Clone)]
pub enum TableElements {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Box<[vm::Anyfunc]>),
}

#[derive(Debug)]
pub struct TableBacking {
    pub elements: TableElements,
    pub max: Option<u32>,
}

impl TableBacking {
    pub fn new(table: &Table) -> Self {
        match table.ty {
            ElementType::Anyfunc => {
                Self {
                    elements: TableElements::Anyfunc(vec![vm::Anyfunc::null(); table.min as usize].into_boxed_slice()),
                    max: table.max,
                }
            }
        }
    }

    pub fn into_vm_table(&mut self) -> vm::LocalTable {
        match self.elements {
            TableElements::Anyfunc(ref mut funcs) => {
                vm::LocalTable {
                    base: funcs.as_mut_ptr() as *mut u8,
                    current_elements: funcs.len(),
                }
            },
        }
    }
}