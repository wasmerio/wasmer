use crate::{
    error::{ExportError, RuntimeError},
    get_global_store, new,
    types::{TableDescriptor, Value},
};

/// Container with a descriptor and a reference to a table storage.
#[derive(Clone)]
pub struct Table {
    new_table: new::wasmer::Table,
}

impl Table {
    /// Create a new `Table` from a [`TableDescriptor`]
    ///
    /// [`TableDescriptor`]: struct.TableDescriptor.html
    ///
    /// # Usage
    ///
    /// ```
    /// # use wasmer_runtime_core::{types::{TableDescriptor, Type, Value}, table::Table, error::RuntimeError};
    /// # fn create_table() -> Result<(), RuntimeError> {
    /// let descriptor = TableDescriptor {
    ///     ty: Type::ExternRef,
    ///     minimum: 10,
    ///     maximum: None,
    /// };
    ///
    /// let table = Table::new(descriptor, Value::null())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(descriptor: TableDescriptor, initial_value: Value) -> Result<Self, RuntimeError> {
        Ok(Self {
            new_table: new::wasmer::Table::new(&get_global_store(), descriptor, initial_value)?,
        })
    }

    /// Get the `TableDescriptor` used to create this `Table`.
    pub fn descriptor(&self) -> TableDescriptor {
        self.new_table.ty().clone()
    }

    /// Set the element at index.
    pub fn set(&self, index: u32, value: Value) -> Result<(), RuntimeError> {
        self.new_table.set(index, value)
    }

    pub fn get(&self, index: u32) -> Option<Value> {
        self.new_table.get(index)
    }

    /// The current size of this table.
    pub fn size(&self) -> u32 {
        self.new_table.size()
    }

    /// Grow this table by `delta`.
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
    fn to_export(&self) -> new::wasmer::Export {
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

#[cfg(test)]
mod table_tests {
    use super::{Table, TableDescriptor};
    use crate::types::{Type, Value};

    #[test]
    fn test_initial_table_size() {
        let table = Table::new(
            TableDescriptor {
                ty: Type::FuncRef,
                minimum: 10,
                maximum: Some(20),
            },
            Value::FuncRef(None),
        )
        .unwrap();
        assert_eq!(table.size(), 10);
    }
}
