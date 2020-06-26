use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::Store;
use crate::types::{Val, ValFuncRef};
use crate::RuntimeError;
use crate::TableType;
use std::sync::Arc;
use wasmer_runtime::{Export, ExportTable, Table as RuntimeTable, VMCallerCheckedAnyfunc};

/// The `Table` struct is an array-like structure representing a WebAssembly Table,
/// which stores function references.
///
/// A table created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
#[derive(Clone)]
pub struct Table {
    store: Store,
    // If the Table is owned by the Store, not the instance
    owned_by_store: bool,
    exported: ExportTable,
}

fn set_table_item(
    table: &dyn RuntimeTable,
    item_index: u32,
    item: VMCallerCheckedAnyfunc,
) -> Result<(), RuntimeError> {
    table.set(item_index, item).map_err(|e| e.into())
}

impl Table {
    /// Creates a new `Table` with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    pub fn new(store: &Store, ty: TableType, init: Val) -> Result<Table, RuntimeError> {
        let item = init.into_checked_anyfunc(store)?;
        let tunables = store.tunables();
        let table_plan = tunables.table_plan(ty);
        let table = tunables
            .create_table(table_plan)
            .map_err(RuntimeError::new)?;

        let num_elements = table.size();
        for i in 0..num_elements {
            set_table_item(table.as_ref(), i, item.clone())?;
        }

        let definition = table.vmtable();
        Ok(Table {
            store: store.clone(),
            owned_by_store: true,
            exported: ExportTable {
                from: table,
                definition,
            },
        })
    }

    fn table(&self) -> &dyn RuntimeTable {
        &*self.exported.from
    }

    /// Gets the underlying [`TableType`].
    pub fn ty(&self) -> &TableType {
        &self.exported.plan().table
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, index: u32) -> Option<Val> {
        let item = self.table().get(index)?;
        Some(ValFuncRef::from_checked_anyfunc(item, &self.store))
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(&self, index: u32, val: Val) -> Result<(), RuntimeError> {
        let item = val.into_checked_anyfunc(&self.store)?;
        set_table_item(self.table(), index, item)
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self) -> u32 {
        self.table().size()
    }

    /// Grows the size of the `Table` by `delta`, initializating
    /// the elements with the provided `init` value.
    ///
    /// It returns the previous size of the `Table` in case is able
    /// to grow the Table successfully.
    ///
    /// # Errors
    ///
    /// Returns an error if the `delta` is out of bounds for the table.
    pub fn grow(&self, delta: u32, init: Val) -> Result<u32, RuntimeError> {
        let item = init.into_checked_anyfunc(&self.store)?;
        let table = self.table();
        match table.grow(delta) {
            Some(len) => {
                for i in 0..delta {
                    set_table_item(table, len + i, item.clone())?;
                }
                Ok(len)
            }
            None => Err(RuntimeError::new(format!(
                "failed to grow table by `{}`",
                delta
            ))),
        }
    }

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        dst_table: &Table,
        dst_index: u32,
        src_table: &Table,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if !Store::same(&dst_table.store, &src_table.store) {
            return Err(RuntimeError::new(
                "cross-`Store` table copies are not supported",
            ));
        }
        RuntimeTable::copy(
            dst_table.table(),
            src_table.table(),
            dst_index,
            src_index,
            len,
        )
        .map_err(RuntimeError::from_trap)?;
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportTable) -> Table {
        Table {
            store: store.clone(),
            owned_by_store: false,
            exported: wasmer_export,
        }
    }

    /// Returns whether or not these two tables refer to the same data.
    pub fn same(&self, other: &Self) -> bool {
        self.exported.same(&other.exported)
    }
}

impl<'a> Exportable<'a> for Table {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
