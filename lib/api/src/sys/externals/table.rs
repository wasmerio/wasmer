use crate::sys::exports::{ExportError, Exportable};
use crate::sys::externals::Extern;
use crate::sys::store::{AsStoreMut, AsStoreRef};
use crate::sys::RuntimeError;
use crate::sys::TableType;
use crate::{ExternRef, Function, Value};
use wasmer_vm::{InternalStoreHandle, StoreHandle, TableElement, VMExtern, VMTable};

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
    handle: StoreHandle<VMTable>,
}

fn set_table_item(
    table: &mut VMTable,
    item_index: u32,
    item: TableElement,
) -> Result<(), RuntimeError> {
    table.set(item_index, item).map_err(|e| e.into())
}

fn value_to_table_element(
    ctx: &mut impl AsStoreMut,
    val: Value,
) -> Result<wasmer_vm::TableElement, RuntimeError> {
    if !val.is_from_store(ctx) {
        return Err(RuntimeError::new("cannot pass Value across contexts"));
    }
    Ok(match val {
        Value::ExternRef(extern_ref) => {
            wasmer_vm::TableElement::ExternRef(extern_ref.map(|e| e.vm_externref()))
        }
        Value::FuncRef(func_ref) => {
            wasmer_vm::TableElement::FuncRef(func_ref.map(|f| f.vm_funcref(ctx)))
        }
        _ => return Err(RuntimeError::new("val is not reference")),
    })
}

fn value_from_table_element(ctx: &mut impl AsStoreMut, item: wasmer_vm::TableElement) -> Value {
    match item {
        wasmer_vm::TableElement::FuncRef(funcref) => {
            Value::FuncRef(funcref.map(|f| unsafe { Function::from_vm_funcref(ctx, f) }))
        }
        wasmer_vm::TableElement::ExternRef(extern_ref) => {
            Value::ExternRef(extern_ref.map(|e| unsafe { ExternRef::from_vm_externref(ctx, e) }))
        }
    }
}

impl Table {
    /// Creates a new `Table` with the provided [`TableType`] definition.
    ///
    /// All the elements in the table will be set to the `init` value.
    ///
    /// This function will construct the `Table` using the store
    /// [`BaseTunables`][crate::sys::BaseTunables].
    pub fn new(
        mut ctx: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let item = value_to_table_element(&mut ctx, init)?;
        let mut ctx = ctx.as_store_mut();
        let tunables = ctx.tunables();
        let style = tunables.table_style(&ty);
        let mut table = tunables
            .create_host_table(&ty, &style)
            .map_err(RuntimeError::new)?;

        let num_elements = table.size();
        for i in 0..num_elements {
            set_table_item(&mut table, i, item.clone())?;
        }

        Ok(Self {
            handle: StoreHandle::new(ctx.objects_mut(), table),
        })
    }

    /// Returns the [`TableType`] of the `Table`.
    pub fn ty(&self, ctx: &impl AsStoreRef) -> TableType {
        *self.handle.get(ctx.as_store_ref().objects()).ty()
    }

    /// Retrieves an element of the table at the provided `index`.
    pub fn get(&self, ctx: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        let item = self.handle.get(ctx.as_store_ref().objects()).get(index)?;
        Some(value_from_table_element(ctx, item))
    }

    /// Sets an element `val` in the Table at the provided `index`.
    pub fn set(
        &self,
        ctx: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        let item = value_to_table_element(ctx, val)?;
        set_table_item(self.handle.get_mut(ctx.objects_mut()), index, item)
    }

    /// Retrieves the size of the `Table` (in elements)
    pub fn size(&self, ctx: &impl AsStoreRef) -> u32 {
        self.handle.get(ctx.as_store_ref().objects()).size()
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
        ctx: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        let item = value_to_table_element(ctx, init)?;
        self.handle
            .get_mut(ctx.objects_mut())
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
        ctx: &mut impl AsStoreMut,
        dst_table: &Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if dst_table.handle.store_id() != src_table.handle.store_id() {
            return Err(RuntimeError::new(
                "cross-`Context` table copies are not supported",
            ));
        }
        let ctx = ctx;
        if dst_table.handle.internal_handle() == src_table.handle.internal_handle() {
            let table = dst_table.handle.get_mut(ctx.objects_mut());
            table.copy_within(dst_index, src_index, len)
        } else {
            let (src_table, dst_table) = ctx.objects_mut().get_2_mut(
                src_table.handle.internal_handle(),
                dst_table.handle.internal_handle(),
            );
            VMTable::copy(dst_table, src_table, dst_index, src_index, len)
        }
        .map_err(RuntimeError::from_trap)?;
        Ok(())
    }

    pub(crate) fn from_vm_extern(
        ctx: &mut impl AsStoreMut,
        internal: InternalStoreHandle<VMTable>,
    ) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(ctx.as_store_ref().objects().id(), internal)
            },
        }
    }

    /// Checks whether this `Table` can be used with the given context.
    pub fn is_from_store(&self, ctx: &impl AsStoreRef) -> bool {
        self.handle.store_id() == ctx.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Table(self.handle.internal_handle())
    }
}

impl std::cmp::PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Table {}

impl<'a> Exportable<'a> for Table {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
