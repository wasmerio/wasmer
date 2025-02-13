//! Data types, functions and traits for `sys` runtime's `Table` implementation.
use crate::{
    backend::sys::entities::engine::NativeEngineExt,
    entities::store::{AsStoreMut, AsStoreRef},
    error::RuntimeError,
    vm::{VMExtern, VMExternTable},
    BackendTable, ExternRef, Function, Value,
};
use wasmer_types::TableType;
use wasmer_vm::{StoreHandle, TableElement, Trap, VMTable};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
/// A WebAssembly `table` in the `sys` runtime.
pub struct Table {
    handle: StoreHandle<VMTable>,
}

fn set_table_item(
    table: &mut VMTable,
    item_index: u32,
    item: TableElement,
) -> Result<(), RuntimeError> {
    table
        .set(item_index, item)
        .map_err(Into::<RuntimeError>::into)
}

fn value_to_table_element(
    store: &mut impl AsStoreMut,
    val: Value,
) -> Result<wasmer_vm::TableElement, RuntimeError> {
    if !val.is_from_store(store) {
        return Err(RuntimeError::new("cannot pass Value across contexts"));
    }
    Ok(match val {
        Value::ExternRef(extern_ref) => {
            wasmer_vm::TableElement::ExternRef(extern_ref.map(|e| e.vm_externref().into_sys()))
        }
        Value::FuncRef(func_ref) => {
            wasmer_vm::TableElement::FuncRef(func_ref.map(|f| f.vm_funcref(store).into_sys()))
        }
        _ => return Err(RuntimeError::new("val is not reference")),
    })
}

fn value_from_table_element(store: &mut impl AsStoreMut, item: wasmer_vm::TableElement) -> Value {
    match item {
        wasmer_vm::TableElement::FuncRef(funcref) => Value::FuncRef(
            funcref
                .map(|f| unsafe { Function::from_vm_funcref(store, crate::vm::VMFuncRef::Sys(f)) }),
        ),
        wasmer_vm::TableElement::ExternRef(extern_ref) => {
            Value::ExternRef(extern_ref.map(|e| unsafe {
                ExternRef::from_vm_externref(store, crate::vm::VMExternRef::Sys(e))
            }))
        }
    }
}

impl Table {
    pub(crate) fn new(
        mut store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let item = value_to_table_element(&mut store, init)?;
        let mut store = store.as_store_mut();
        let tunables = store.engine().tunables();
        let style = tunables.table_style(&ty);
        let mut table = tunables
            .create_host_table(&ty, &style)
            .map_err(RuntimeError::new)?;

        let num_elements = table.size();
        for i in 0..num_elements {
            set_table_item(&mut table, i, item.clone())?;
        }

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), table),
        })
    }

    pub(crate) fn ty(&self, store: &impl AsStoreRef) -> TableType {
        *self
            .handle
            .get(store.as_store_ref().objects().as_sys())
            .ty()
    }

    pub(crate) fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        let item = self
            .handle
            .get(store.as_store_ref().objects().as_sys())
            .get(index)?;
        Some(value_from_table_element(store, item))
    }

    pub(crate) fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        let item = value_to_table_element(store, val)?;
        set_table_item(
            self.handle.get_mut(store.objects_mut().as_sys_mut()),
            index,
            item,
        )
    }

    pub(crate) fn size(&self, store: &impl AsStoreRef) -> u32 {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .size()
    }

    pub(crate) fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        let item = value_to_table_element(store, init)?;
        let obj_mut = store.objects_mut().as_sys_mut();

        self.handle
            .get_mut(obj_mut)
            .grow(delta, item)
            .ok_or_else(|| RuntimeError::new(format!("failed to grow table by `{delta}`")))
    }

    pub(crate) fn copy(
        store: &mut impl AsStoreMut,
        dst_table: &Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if dst_table.handle.store_id() != src_table.handle.store_id() {
            return Err(RuntimeError::new(
                "cross-`Store` table copies are not supported",
            ));
        }
        if dst_table.handle.internal_handle() == src_table.handle.internal_handle() {
            let table = dst_table.handle.get_mut(store.objects_mut().as_sys_mut());
            table.copy_within(dst_index, src_index, len)
        } else {
            let (src_table, dst_table) = store.objects_mut().as_sys_mut().get_2_mut(
                src_table.handle.internal_handle(),
                dst_table.handle.internal_handle(),
            );
            VMTable::copy(dst_table, src_table, dst_index, src_index, len)
        }
        .map_err(Into::<RuntimeError>::into)?;
        Ok(())
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTable) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(
                    store.as_store_ref().objects().id(),
                    vm_extern.into_sys(),
                )
            },
        }
    }

    /// Checks whether this `Table` can be used with the given context.
    pub(crate) fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Sys(wasmer_vm::VMExtern::Table(self.handle.internal_handle()))
    }
}

impl std::cmp::PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Table {}

impl crate::Table {
    /// Consume [`self`] into [`crate::backend::sys::table::Table`].
    pub fn into_sys(self) -> crate::backend::sys::table::Table {
        match self.0 {
            BackendTable::Sys(s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::sys::table::Table`].
    pub fn as_sys(&self) -> &crate::backend::sys::table::Table {
        match self.0 {
            BackendTable::Sys(ref s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::sys::table::Table`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::table::Table {
        match self.0 {
            BackendTable::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }
}

impl crate::BackendTable {
    /// Consume [`self`] into [`crate::backend::sys::table::Table`].
    pub fn into_sys(self) -> crate::backend::sys::table::Table {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::sys::table::Table`].
    pub fn as_sys(&self) -> &crate::backend::sys::table::Table {
        match self {
            Self::Sys(ref s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::sys::table::Table`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::table::Table {
        match self {
            Self::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` table!"),
        }
    }
}
