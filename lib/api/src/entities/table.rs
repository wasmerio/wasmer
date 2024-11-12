use wasmer_types::TableType;

use crate::{
    error::RuntimeError,
    vm::{VMExtern, VMExternTable},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, StoreMut, StoreRef, Value,
};

/// A WebAssembly `table` instance.
///
/// The `Table` struct is an array-like structure representing a WebAssembly Table,
/// which stores function references.
///
/// A table created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#table-instances>
#[derive(Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Table(pub(crate) Box<dyn TableLike>);

impl Table {
    /// Creates a new table with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the table using the store `BaseTunables`.
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        Ok(Self(store.as_store_mut().table_from_value(ty, init)?))
    }

    /// Returns the [`TableType`] of the table.
    pub fn ty(&self, store: &impl AsStoreRef) -> TableType {
        self.0.ty(store.as_store_ref())
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        self.0.get(store.as_store_mut(), index)
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        self.0.set(store.as_store_mut(), index, val)
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        self.0.size(store.as_store_ref())
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
    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        self.0.grow(store.as_store_mut(), delta, init)
    }

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        store: &mut impl AsStoreMut,
        dst_table: &Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        store.as_store_mut().copy(
            dst_table.0.as_ref(),
            dst_index,
            src_table.0.as_ref(),
            src_index,
            len,
        )
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, ext: VMExternTable) -> Self {
        Self(store.as_store_mut().table_from_vm_extern(ext))
    }

    /// Checks whether this `Table` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store.as_store_ref())
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl std::cmp::PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

impl std::cmp::Eq for Table {}

impl Clone for Table {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl<'a> Exportable<'a> for Table {
    fn get_self_from_extern(ext: &'a Extern) -> Result<&'a Self, ExportError> {
        match ext {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// The trait that every concrete table must implement.
pub trait TableLike: std::fmt::Debug {
    /// Returns the [`TableType`] of the table.
    fn ty(&self, store: StoreRef) -> TableType;

    /// Retrieves an element of the table at the provided `index`.
    fn get(&self, store: StoreMut, index: u32) -> Option<Value>;

    /// Retrieves the size of the table, as count of current elements.
    fn size(&self, store: StoreRef) -> u32;

    /// Sets an element `val` in the Table at the provided `index`.
    fn set(&self, store: StoreMut, index: u32, val: Value) -> Result<(), RuntimeError>;

    /// Grows the size of the table by `delta`, initializating
    /// the elements with the provided `init` value.
    ///
    /// It returns the previous size of the table in case is able
    /// to grow the Table successfully.
    ///
    /// # Errors
    ///
    /// Returns an error if the `delta` is out of bounds for the table.
    fn grow(&self, store: StoreMut, delta: u32, init: Value) -> Result<u32, RuntimeError>;

    /// Checks whether this table can be used with the given context.
    fn is_from_store(&self, store: StoreRef) -> bool;

    /// Create a [`VMExtern`] from self.
    fn to_vm_extern(&self) -> VMExtern;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn TableLike>;
}

/// The trait implemented by all those that can create new tables.
pub trait TableCreator {
    /// Creates a new table with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    fn table_from_value(
        &mut self,
        ty: TableType,
        init: Value,
    ) -> Result<Box<dyn TableLike>, RuntimeError>;

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    fn copy(
        &mut self,
        dst_table: &dyn TableLike,
        dst_index: u32,
        src_table: &dyn TableLike,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError>;

    /// Create a new table from a [`VMExternTable`].
    fn table_from_vm_extern(&mut self, ext: VMExternTable) -> Box<dyn TableLike>;
}

#[cfg(test)]
mod test {
    /// Check the example from <https://github.com/wasmerio/wasmer/issues/3197>.
    #[test]
    #[cfg_attr(
        feature = "wamr",
        ignore = "wamr does not support direct calls to grow table"
    )]
    #[cfg_attr(feature = "wasmi", ignore = "wasmi does not support funcrefs")]
    #[cfg_attr(
        feature = "v8",
        ignore = "growing tables in v8 is not currently supported"
    )]
    fn table_grow_issue_3197() {
        use crate::{imports, Instance, Module, Store, Table, TableType, Type, Value};

        const WAT: &str = r#"(module (table (import "env" "table") 100 funcref))"#;

        // Tests that the table type of `table` is compatible with the export in the WAT
        // This tests that `wasmer_types::types::is_table_compatible` works as expected.
        let mut store = Store::default();
        let module = Module::new(&store, WAT).unwrap();
        let ty = TableType::new(Type::FuncRef, 0, None);
        let table = Table::new(&mut store, ty, Value::FuncRef(None)).unwrap();
        table.grow(&mut store, 100, Value::FuncRef(None)).unwrap();
        assert_eq!(table.ty(&store).minimum, 0);
        let imports = imports! {"env" => {"table" => table}};
        let _instance = Instance::new(&mut store, &module, &imports).unwrap();
    }
}
