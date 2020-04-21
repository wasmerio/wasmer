//! The runtime table module contains data structures and functions used to create and update wasm
//! tables.
use crate::{
    error::CreationError,
    export::Export,
    import::IsExport,
    types::{ElementType, TableType},
    vm,
};
use std::{
    convert::TryFrom,
    fmt, ptr,
    sync::{Arc, Mutex},
};

mod anyfunc;

pub use self::anyfunc::Anyfunc;
pub(crate) use self::anyfunc::AnyfuncTable;
use crate::error::GrowError;

/// Error type indicating why a table access failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableAccessError {
    /// The index wasn't valid, so no element could be accessed.
    IndexError,

    // we'll need this error when we support tables holding more types
    #[allow(dead_code)]
    /// The type of the table was incorrect, so no element could be accessed.
    TypeError,
}

/// Trait indicates types that can be stored in tables
pub trait StorableInTable: Sized {
    /// Attempt to lookup self in the given table.
    fn unwrap_self(storage: &TableStorage, index: u32) -> Result<Self, TableAccessError>;

    /// Wrap value to be stored in a table.
    fn wrap_self(self, storage: &mut TableStorage, index: u32) -> Result<(), TableAccessError>;
}

impl<'a, F: Into<Anyfunc<'a>> + TryFrom<Anyfunc<'a>>> StorableInTable for F {
    fn unwrap_self(storage: &TableStorage, index: u32) -> Result<Self, TableAccessError> {
        match storage {
            TableStorage::Anyfunc(ref anyfunc_table) => {
                let anyfunc = anyfunc_table
                    .get(index)
                    .ok_or(TableAccessError::IndexError)?;
                // Should this be a different error value because it's not a table type error?
                F::try_from(anyfunc).map_err(|_| TableAccessError::TypeError)
            }
        }
    }

    fn wrap_self(self, storage: &mut TableStorage, index: u32) -> Result<(), TableAccessError> {
        let anyfunc: Anyfunc = self.into();

        match storage {
            TableStorage::Anyfunc(ref mut anyfunc_table) => anyfunc_table
                .set(index, anyfunc)
                .map_err(|_| TableAccessError::IndexError),
        }
    }
}

/// Kind of table element.
// note to implementors: all types in `Element` should implement `StorableInTable`.
pub enum Element<'a> {
    /// Anyfunc.
    Anyfunc(Anyfunc<'a>),
}

// delegation implementation for `Element`
impl<'a> StorableInTable for Element<'a> {
    fn unwrap_self(storage: &TableStorage, index: u32) -> Result<Self, TableAccessError> {
        match storage {
            TableStorage::Anyfunc(ref anyfunc_table) => anyfunc_table
                .get(index)
                .map(Element::Anyfunc)
                .ok_or(TableAccessError::IndexError),
        }
    }

    fn wrap_self(self, storage: &mut TableStorage, index: u32) -> Result<(), TableAccessError> {
        match self {
            Element::Anyfunc(af) => af.wrap_self(storage, index),
        }
    }
}

/// Kind of table storage.
// #[derive(Debug)]
pub enum TableStorage {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Box<AnyfuncTable>),
}

/// Container with a descriptor and a reference to a table storage.
pub struct Table {
    desc: TableType,
    storage: Arc<Mutex<(TableStorage, vm::LocalTable)>>,
}

impl Table {
    /// Create a new `Table` from a [`TableType`]
    ///
    /// [`TableType`]: struct.TableType.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::types::{TableType, ElementType};
    /// # use wasmer_runtime_core::table::Table;
    /// # use wasmer_runtime_core::error::Result;
    /// # fn create_table() -> Result<()> {
    /// let descriptor = TableType {
    ///     element: ElementType::Anyfunc,
    ///     minimum: 10,
    ///     maximum: None,
    /// };
    ///
    /// let table = Table::new(descriptor)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(desc: TableType) -> Result<Self, CreationError> {
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

    /// Get the `TableType` used to create this `Table`.
    pub fn descriptor(&self) -> TableType {
        self.desc
    }

    // Made `pub(crate)` because this API is incomplete, see `anyfunc::AnyfuncTable::get`
    // for more information.
    #[allow(dead_code)]
    /// Get the raw table value at index. A return value of `None` means either that
    /// the index or the type wasn't valid.
    pub(crate) fn get<T: StorableInTable>(&self, index: u32) -> Result<T, TableAccessError> {
        let guard = self.storage.lock().unwrap();
        let (storage, _) = &*guard;
        T::unwrap_self(storage, index)
    }

    /// Set the element at index.
    pub fn set<T: StorableInTable>(&self, index: u32, element: T) -> Result<(), TableAccessError> {
        let mut guard = self.storage.lock().unwrap();
        let (storage, _) = &mut *guard;
        T::wrap_self(element, storage, index)
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

    use super::{ElementType, Table, TableType};

    #[test]
    fn test_initial_table_size() {
        let table = Table::new(TableType {
            element: ElementType::Anyfunc,
            minimum: 10,
            maximum: Some(20),
        })
        .unwrap();
        assert_eq!(table.size(), 10);
    }
}
