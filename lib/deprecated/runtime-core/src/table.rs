use crate::{
    error::{ExportError, RuntimeError},
    get_global_store, new,
    types::{TableDescriptor, Value},
};

#[derive(Clone)]
pub struct Table {
    new_table: new::wasmer::Table,
}

impl Table {
    pub fn new(descriptor: TableDescriptor, initial_value: Value) -> Result<Self, RuntimeError> {
        Ok(Self {
            new_table: new::wasmer::Table::new(get_global_store(), descriptor, initial_value)?,
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

impl From<&new::wasmer::Table> for Table {
    fn from(new_table: &new::wasmer::Table) -> Self {
        Self {
            new_table: new_table.clone(),
        }
    }
}

impl<'a> new::wasmer::Exportable<'a> for Table {
    fn to_export(&self) -> new::wasmer_runtime::Export {
        self.new_table.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Table(table) => Ok(
                // It's not ideal to call `Box::leak` here, but it
                // would introduce too much changes in the
                // `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<Table>::new(table.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
