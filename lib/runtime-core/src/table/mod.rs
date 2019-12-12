//! The runtime table module contains data structures and functions used to create and update wasm
//! tables.
use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    types::{ElementType, TableDescriptor},
    vm,
};
use std::{
    fmt, ptr,
    sync::{Arc, Mutex},
};

mod anyfunc;

pub use self::anyfunc::Anyfunc;
pub(crate) use self::anyfunc::AnyfuncTable;
use crate::error::GrowError;

/// Kind of table element.
pub enum Element<'a> {
    /// Anyfunc.
    Anyfunc(Anyfunc<'a>),
}

/// Kind of table storage.
// #[derive(Debug)]
pub enum TableStorage {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Box<AnyfuncTable>),
}

/// Container with a descriptor and a reference to a table storage.
pub struct Table {
    desc: TableDescriptor,
    storage: Arc<Mutex<(TableStorage, vm::LocalTable)>>,
}

impl Table {
    /// Create a new `Table` from a [`TableDescriptor`]
    ///
    /// [`TableDescriptor`]: struct.TableDescriptor.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::types::{TableDescriptor, ElementType};
    /// # use wasmer_runtime_core::table::Table;
    /// # use wasmer_runtime_core::error::Result;
    /// # fn create_table() -> Result<()> {
    /// let descriptor = TableDescriptor {
    ///     element: ElementType::Anyfunc,
    ///     minimum: 10,
    ///     maximum: None,
    /// };
    ///
    /// let table = Table::new(descriptor)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(desc: TableDescriptor) -> Result<Self, CreationError> {
        if let Some(max) = desc.maximum {
            if max < desc.minimum {
                return Err(CreationError::InvalidDescriptor(
                    "Max table size is less than the minimum size".to_string(),
                ));
            }
        }

        let mut local = vm::LocalTable {
            base: ptr::null_mut(),
            count: 0,
            table: ptr::null_mut(),
        };

        let storage = match desc.element {
            ElementType::Anyfunc => TableStorage::Anyfunc(AnyfuncTable::new(desc, &mut local)?),
        };

        Ok(Self {
            desc,
            storage: Arc::new(Mutex::new((storage, local))),
        })
    }

    /// Get the `TableDescriptor` used to create this `Table`.
    pub fn descriptor(&self) -> TableDescriptor {
        self.desc
    }

    /// Set the element at index.
    pub fn set(&self, index: u32, element: Element) -> Result<(), ()> {
        let mut storage = self.storage.lock().unwrap();
        match &mut *storage {
            (TableStorage::Anyfunc(ref mut anyfunc_table), _) => {
                match element {
                    Element::Anyfunc(anyfunc) => anyfunc_table.set(index, anyfunc),
                    // _ => panic!("wrong element type for anyfunc table"),
                }
            }
        }
    }

    pub(crate) fn anyfunc_direct_access_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut [vm::Anyfunc]) -> R,
    {
        let mut storage = self.storage.lock().unwrap();
        match &mut *storage {
            (TableStorage::Anyfunc(ref mut anyfunc_table), _) => f(anyfunc_table.internal_buffer()),
        }
    }

    /// The current size of this table.
    pub fn size(&self) -> u32 {
        let storage = self.storage.lock().unwrap();
        match &*storage {
            (TableStorage::Anyfunc(ref anyfunc_table), _) => anyfunc_table.current_size(),
        }
    }

    /// Grow this table by `delta`.
    pub fn grow(&self, delta: u32) -> Result<u32, GrowError> {
        if delta == 0 {
            return Ok(self.size());
        }

        let mut storage = self.storage.lock().unwrap();
        match &mut *storage {
            (TableStorage::Anyfunc(ref mut anyfunc_table), ref mut local) => anyfunc_table
                .grow(delta, local)
                .ok_or(GrowError::TableGrowError),
        }
    }

    /// Get a mutable pointer to underlying table storage.
    pub fn vm_local_table(&mut self) -> *mut vm::LocalTable {
        let mut storage = self.storage.lock().unwrap();
        &mut storage.1
    }
}

impl IsExport for Table {
    fn to_export(&self) -> Export {
        Export::Table(self.clone())
    }
}

impl Clone for Table {
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            storage: Arc::clone(&self.storage),
        }
    }
}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Table")
            .field("desc", &self.desc)
            .field("size", &self.size())
            .finish()
    }
}

#[cfg(test)]
mod table_tests {

    use super::{ElementType, Table, TableDescriptor};

    #[test]
    fn test_initial_table_size() {
        let table = Table::new(TableDescriptor {
            element: ElementType::Anyfunc,
            minimum: 10,
            maximum: Some(20),
        })
        .unwrap();
        assert_eq!(table.size(), 10);
    }
}
