use crate::{
    error::RuntimeError,
    new,
    types::{TableDescriptor, Value},
};

pub struct Table {
    new_table: new::wasmer::Table,
}

impl Table {
    pub fn new(descriptor: TableDescriptor, initial_value: Value) -> Result<Self, RuntimeError> {
        let store = Default::default();

        Ok(Self {
            new_table: new::wasmer::Table::new(&store, descriptor, initial_value)?,
        })
    }

    pub fn descriptor(&self) -> TableDescriptor {
        self.new_table.ty().clone()
    }

    pub fn set(&self, index: u32, value: Value) -> Result<(), RuntimeError> {
        self.new_table.set(index, value)
    }

    pub fn get(&self, index: u32) -> Option<Value> {
        self.new_table.get(index)
    }

    pub fn size(&self) -> u32 {
        self.new_table.size()
    }

    pub fn grow(&self, delta: u32, initial_value: Value) -> Result<u32, RuntimeError> {
        self.new_table.grow(delta, initial_value)
    }
}
