use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::Store;
use crate::types::{Val, ValFuncRef};
use crate::RuntimeError;
use crate::TableType;
use loupe::MemoryUsage;
use std::sync::Arc;
use wasmer_engine::{Export, ExportTable};
use wasmer_vm::{Table as RuntimeTable, TableElement, VMExportTable};

/// A WebAssembly `table` instance.
///
/// The `Table` struct is an array-like structure representing a WebAssembly Table,
/// which stores function references.
///
/// A table created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#table-instances>
#[derive(Clone, MemoryUsage)]
pub struct Table {
    store: Store,
    table: Arc<dyn RuntimeTable>,
}

fn set_table_item(
    table: &dyn RuntimeTable,
    item_index: u32,
    item: TableElement,
) -> Result<(), RuntimeError> {
    table.set(item_index, item).map_err(|e| e.into())
}

impl Table {
    /// Creates a new `Table` with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the `Table` using the store
    /// [`BaseTunables`][crate::tunables::BaseTunables].
    pub fn new(store: &Store, ty: TableType, init: Val) -> Result<Self, RuntimeError> {
        let item = init.into_table_reference(store)?;
        let tunables = store.tunables();
        let style = tunables.table_style(&ty);
        let table = tunables
            .create_host_table(&ty, &style)
            .map_err(RuntimeError::new)?;

        let num_elements = table.size();
        for i in 0..num_elements {
            set_table_item(table.as_ref(), i, item.clone())?;
        }

        Ok(Self {
            store: store.clone(),
            table,
        })
    }

    /// Returns the [`TableType`] of the `Table`.
    pub fn ty(&self) -> &TableType {
        self.table.ty()
    }

    /// Returns the [`Store`] where the `Table` belongs.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, index: u32) -> Option<Val> {
        let item = self.table.get(index)?;
        Some(ValFuncRef::from_table_reference(item, &self.store))
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(&self, index: u32, val: Val) -> Result<(), RuntimeError> {
        let item = val.into_table_reference(&self.store)?;
        set_table_item(self.table.as_ref(), index, item)
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self) -> u32 {
        self.table.size()
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
        let item = init.into_table_reference(&self.store)?;
        self.table
            .grow(delta, item)
            .ok_or_else(|| RuntimeError::new(format!("failed to grow table by `{}`", delta)))
    }

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        dst_table: &Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if !Store::same(&dst_table.store, &src_table.store) {
            return Err(RuntimeError::new(
                "cross-`Store` table copies are not supported",
            ));
        }
        RuntimeTable::copy(
            dst_table.table.as_ref(),
            src_table.table.as_ref(),
            dst_index,
            src_index,
            len,
        )
        .map_err(RuntimeError::from_trap)?;
        Ok(())
    }

    pub(crate) fn from_vm_export(store: &Store, wasmer_export: ExportTable) -> Self {
        Self {
            store: store.clone(),
            table: wasmer_export.vm_table.from,
        }
    }

    /// Returns whether or not these two tables refer to the same data.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.table, &other.table)
    }
}

impl<'a> Exportable<'a> for Table {
    fn to_export(&self) -> Export {
        ExportTable {
            vm_table: VMExportTable {
                from: self.table.clone(),
                instance_ref: None,
            },
        }
        .into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
