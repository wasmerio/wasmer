use crate::js::export::{VMFunction, VMTable};
use crate::js::exports::{ExportError, Exportable};
use crate::js::externals::{Extern, VMExtern};
use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle, StoreHandle};
use crate::js::value::Value;
use crate::js::RuntimeError;
use crate::js::{FunctionType, TableType};
use js_sys::Function;

/// A WebAssembly `table` instance.
///
/// The `Table` struct is an array-like structure representing a WebAssembly Table,
/// which stores function references.
///
/// A table created by the host or in WebAssembly code will be accessible and
/// mutable from both host and WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#table-instances>
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub(crate) handle: StoreHandle<VMTable>,
}

fn set_table_item(table: &VMTable, item_index: u32, item: &Function) -> Result<(), RuntimeError> {
    table.table.set(item_index, item).map_err(|e| e.into())
}

fn get_function(store: &mut impl AsStoreMut, val: Value) -> Result<Function, RuntimeError> {
    if !val.is_from_store(store) {
        return Err(RuntimeError::new("cannot pass Value across contexts"));
    }
    match val {
        Value::FuncRef(Some(ref func)) => Ok(func
            .handle
            .get(&store.as_store_ref().objects())
            .function
            .clone()
            .into()),
        // Only funcrefs is supported by the spec atm
        _ => unimplemented!(),
    }
}

impl Table {
    /// Creates a new `Table` with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the `Table` using the store
    /// [`BaseTunables`][crate::js::tunables::BaseTunables].
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let mut store = store;
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"initial".into(), &ty.minimum.into())?;
        if let Some(max) = ty.maximum {
            js_sys::Reflect::set(&descriptor, &"maximum".into(), &max.into())?;
        }
        js_sys::Reflect::set(&descriptor, &"element".into(), &"anyfunc".into())?;

        let js_table = js_sys::WebAssembly::Table::new(&descriptor)?;
        let table = VMTable::new(js_table, ty);

        let num_elements = table.table.length();
        let func = get_function(&mut store, init)?;
        for i in 0..num_elements {
            set_table_item(&table, i, &func)?;
        }

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut(), table),
        })
    }

    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Table(self.handle.internal_handle())
    }

    /// Returns the [`TableType`] of the `Table`.
    pub fn ty(&self, store: &impl AsStoreRef) -> TableType {
        self.handle.get(store.as_store_ref().objects()).ty
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        if let Some(func) = self
            .handle
            .get(store.as_store_ref().objects())
            .table
            .get(index)
            .ok()
        {
            let ty = FunctionType::new(vec![], vec![]);
            let vm_function = VMFunction::new(func, ty);
            let function = crate::js::externals::Function::from_vm_export(store, vm_function);
            Some(Value::FuncRef(Some(function)))
        } else {
            None
        }
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        let item = get_function(store, val)?;
        set_table_item(self.handle.get_mut(store.objects_mut()), index, &item)
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        self.handle
            .get(store.as_store_ref().objects())
            .table
            .length()
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
        _store: &mut impl AsStoreMut,
        _delta: u32,
        _init: Value,
    ) -> Result<u32, RuntimeError> {
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
        _store: &mut impl AsStoreMut,
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        unimplemented!("Table.copy is not natively supported in Javascript");
    }

    pub(crate) fn from_vm_extern(
        store: &mut impl AsStoreMut,
        internal: InternalStoreHandle<VMTable>,
    ) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(store.as_store_ref().objects().id(), internal)
            },
        }
    }

    /// Checks whether this `Table` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    /// Get access to the backing VM value for this extern. This function is for
    /// tests it should not be called by users of the Wasmer API.
    ///
    /// # Safety
    /// This function is unsafe to call outside of tests for the wasmer crate
    /// because there is no stability guarantee for the returned type and we may
    /// make breaking changes to it at any time or remove this method.
    #[doc(hidden)]
    pub unsafe fn get_vm_table<'context>(
        &self,
        store: &'context impl AsStoreRef,
    ) -> &'context VMTable {
        self.handle.get(store.as_store_ref().objects())
    }
}

impl<'a> Exportable<'a> for Table {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
