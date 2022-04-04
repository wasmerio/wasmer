use crate::wasm::export::{Export, VMTable};
use crate::wasm::exports::{ExportError, Exportable};
use crate::wasm::externals::Extern;
use crate::wasm::store::Store;
use crate::wasm::types::Val;
use crate::wasm::RuntimeError;
use crate::wasm::TableType;

/// A WebAssembly `table` instance.
///
/// The `Table` struct is an array-like structure representing a WebAssembly Table,
/// which stores function references.
///
/// A table created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#table-instances>
#[derive(Debug, Clone)]
pub struct Table {
    store: Store,
    vm_table: VMTable,
}

impl Table {
    /// Creates a new `Table` with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the `Table` using the store
    /// [`BaseTunables`][crate::js::tunables::BaseTunables].
    pub fn new(_store: &Store, _ty: TableType, _init: Val) -> Result<Self, RuntimeError> {
        panic!("Not implemented!")
    }

    /// Returns the [`TableType`] of the `Table`.
    pub fn ty(&self) -> &TableType {
        &self.vm_table.ty()
    }

    /// Returns the [`Store`] where the `Table` belongs.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, _index: u32) -> Option<Val> {
        panic!("Not implemented!")
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(&self, _index: u32, _val: Val) -> Result<(), RuntimeError> {
        panic!("Not implemented!")
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self) -> u32 {
        panic!("Not implemented!")
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
    pub fn grow(&self, _delta: u32, _init: Val) -> Result<u32, RuntimeError> {
        unimplemented!();
    }

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        unimplemented!("Table.copy is not natively supported in Javascript");
    }

    pub(crate) fn from_vm_export(store: &Store, vm_table: VMTable) -> Self {
        Self {
            store: store.clone(),
            vm_table,
        }
    }

    /// Returns whether or not these two tables refer to the same data.
    pub fn same(&self, _other: &Self) -> bool {
        panic!("Not implemented!")
    }

    /// Get access to the backing VM value for this extern. This function is for
    /// tests it should not be called by users of the Wasmer API.
    ///
    /// # Safety
    /// This function is unsafe to call outside of tests for the wasmer crate
    /// because there is no stability guarantee for the returned type and we may
    /// make breaking changes to it at any time or remove this method.
    #[doc(hidden)]
    pub unsafe fn get_vm_table(&self) -> &VMTable {
        &self.vm_table
    }
}

impl<'a> Exportable<'a> for Table {
    fn to_export(&self) -> Export {
        Export::Table(self.vm_table.clone())
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
