use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    types::{ElementType, TableDescriptor},
    vm,
};
use std::{cell::RefCell, fmt, ptr, rc::Rc};

mod anyfunc;

pub use self::anyfunc::Anyfunc;
use self::anyfunc::AnyfuncTable;
use crate::error::GrowError;

pub enum Element<'a> {
    Anyfunc(Anyfunc<'a>),
}

// #[derive(Debug)]
pub enum TableStorage {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Box<AnyfuncTable>),
}

pub struct Table {
    desc: TableDescriptor,
    storage: Rc<RefCell<(TableStorage, vm::LocalTable)>>,
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
            storage: Rc::new(RefCell::new((storage, local))),
        })
    }

    /// Get the `TableDescriptor` used to create this `Table`.
    pub fn descriptor(&self) -> TableDescriptor {
        self.desc
    }

    /// Set the element at index.
    pub fn set(&self, index: u32, element: Element) -> Result<(), ()> {
        match &mut *self.storage.borrow_mut() {
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
        match &mut *self.storage.borrow_mut() {
            (TableStorage::Anyfunc(ref mut anyfunc_table), _) => f(anyfunc_table.internal_buffer()),
        }
    }

    /// The current size of this table.
    pub fn size(&self) -> u32 {
        match &*self.storage.borrow() {
            (TableStorage::Anyfunc(ref anyfunc_table), _) => anyfunc_table.current_size(),
        }
    }

    /// Grow this table by `delta`.
    pub fn grow(&self, delta: u32) -> Result<u32, GrowError> {
        if delta == 0 {
            return Ok(self.size());
        }

        match &mut *self.storage.borrow_mut() {
            (TableStorage::Anyfunc(ref mut anyfunc_table), ref mut local) => anyfunc_table
                .grow(delta, local)
                .ok_or(GrowError::TableGrowError),
        }
    }

    pub fn vm_local_table(&mut self) -> *mut vm::LocalTable {
        &mut self.storage.borrow_mut().1
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
            storage: Rc::clone(&self.storage),
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
