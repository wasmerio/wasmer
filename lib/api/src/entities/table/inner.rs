use wasmer_types::TableType;

use crate::{
    error::RuntimeError,
    macros::backend::{gen_rt_ty, match_rt},
    store::BackendStore,
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
gen_rt_ty!(Table
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq, derive_more::From
);

impl BackendTable {
    /// Creates a new table with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the table using the store `BaseTunables`.
    #[inline]
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            BackendStore::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::table::Table::new(store, ty, init)?,
            )),
            #[cfg(feature = "wamr")]
            BackendStore::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::table::Table::new(store, ty, init)?,
            )),
            #[cfg(feature = "wasmi")]
            BackendStore::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::table::Table::new(store, ty, init)?,
            )),
            #[cfg(feature = "v8")]
            BackendStore::V8(_) => Ok(Self::V8(crate::backend::v8::entities::table::Table::new(
                store, ty, init,
            )?)),
            #[cfg(feature = "js")]
            BackendStore::Js(_) => Ok(Self::Js(crate::backend::js::entities::table::Table::new(
                store, ty, init,
            )?)),
            #[cfg(feature = "jsc")]
            BackendStore::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::table::Table::new(store, ty, init)?,
            )),
        }
    }

    /// Returns the [`TableType`] of the table.
    #[inline]
    pub fn ty(&self, store: &impl AsStoreRef) -> TableType {
        match_rt!(on self => s {
            s.ty(store)
        })
    }

    /// Retrieves an element of the table at the provided `index`.
    #[inline]
    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        match_rt!(on self => s {
            s.get(store, index)
        })
    }

    /// Sets an element `val` in the Table at the provided `index`.
    #[inline]
    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        match_rt!(on self => s {
            s.set(store, index, val)
        })
    }

    /// Retrieves the size of the `Table` (in elements)
    #[inline]
    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        match_rt!(on self => s {
            s.size(store)
        })
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
    #[inline]
    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        match_rt!(on self => s {
            s.grow(store, delta, init)
        })
    }

    /// Copies the `len` elements of `src_table` starting at `src_index`
    /// to the destination table `dst_table` at index `dst_index`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    #[inline]
    pub fn copy(
        store: &mut impl AsStoreMut,
        dst_table: &Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            BackendStore::Sys(_) => crate::backend::sys::entities::table::Table::copy(
                store,
                dst_table.as_sys(),
                dst_index,
                src_table.as_sys(),
                src_index,
                len,
            ),
            #[cfg(feature = "wamr")]
            BackendStore::Wamr(_) => crate::backend::wamr::entities::table::Table::copy(
                store,
                dst_table.as_wamr(),
                dst_index,
                src_table.as_wamr(),
                src_index,
                len,
            ),
            #[cfg(feature = "wasmi")]
            BackendStore::Wasmi(_) => crate::backend::wasmi::entities::table::Table::copy(
                store,
                dst_table.as_wasmi(),
                dst_index,
                src_table.as_wasmi(),
                src_index,
                len,
            ),

            #[cfg(feature = "v8")]
            BackendStore::V8(_) => crate::backend::v8::entities::table::Table::copy(
                store,
                dst_table.as_v8(),
                dst_index,
                src_table.as_v8(),
                src_index,
                len,
            ),
            #[cfg(feature = "js")]
            BackendStore::Js(_) => crate::backend::js::entities::table::Table::copy(
                store,
                dst_table.as_js(),
                dst_index,
                src_table.as_js(),
                src_index,
                len,
            ),
            #[cfg(feature = "jsc")]
            BackendStore::Jsc(_) => crate::backend::jsc::entities::table::Table::copy(
                store,
                dst_table.as_jsc(),
                dst_index,
                src_table.as_jsc(),
                src_index,
                len,
            ),
        }
    }

    #[inline]
    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, ext: VMExternTable) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::entities::table::Table::from_vm_extern(store, ext),
            ),
            #[cfg(feature = "wamr")]
            BackendStore::Wamr(_) => {
                Self::Wamr(crate::backend::wamr::entities::table::Table::from_vm_extern(store, ext))
            }
            #[cfg(feature = "wasmi")]
            BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::table::Table::from_vm_extern(store, ext),
            ),
            #[cfg(feature = "v8")]
            BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::table::Table::from_vm_extern(store, ext),
            ),
            #[cfg(feature = "js")]
            BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::table::Table::from_vm_extern(store, ext),
            ),
            #[cfg(feature = "jsc")]
            BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::table::Table::from_vm_extern(store, ext),
            ),
        }
    }

    /// Checks whether this `Table` can be used with the given context.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => s {
            s.is_from_store(store)
        })
    }

    #[inline]
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match_rt!(on self => s {
            s.to_vm_extern()
        })
    }
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
