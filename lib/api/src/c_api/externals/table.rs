use crate::bindings::{
    wasm_extern_as_ref, wasm_limits_t, wasm_ref_t, wasm_table_new, wasm_table_type,
    wasm_tabletype_element, wasm_tabletype_limits, wasm_tabletype_new, wasm_tabletype_t,
    wasm_val_t, wasm_valtype_new,
};
use crate::c_api::bindings::wasm_table_as_extern;
use crate::c_api::vm::{VMExtern, VMExternTable, VMFunction, VMTable};
use crate::errors::RuntimeError;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::value::Value;
use crate::{as_c, FunctionType, TableType};

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub(crate) handle: VMTable,
}

unsafe impl Send for Table {}
unsafe impl Sync for Table {}

impl Table {
    pub(crate) fn type_to_wamr(ty: TableType) -> *mut wasm_tabletype_t {
        let valtype = unsafe { wasm_valtype_new(as_c::type_to_c(&ty.ty)) };

        let limits = Box::into_raw(Box::new(wasm_limits_t {
            min: ty.minimum,
            max: match ty.maximum {
                Some(v) => v,
                None => 0,
            },
        }));

        unsafe { wasm_tabletype_new(valtype, limits) }
    }

    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();

        let wasm_tablety = Self::type_to_wamr(ty);
        let init: wasm_val_t = as_c::result_to_value(&init);

        Ok(Self {
            handle: unsafe {
                wasm_table_new(store_mut.inner.store.inner, wasm_tablety, init.of.ref_)
            },
        })
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        unsafe { wasm_table_as_extern(self.handle) }
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> TableType {
        let wamr_table_type: *mut wasm_tabletype_t = unsafe { wasm_table_type(self.handle) };
        let table_limits = unsafe { wasm_tabletype_limits(wamr_table_type) };
        let table_type = unsafe { wasm_tabletype_element(wamr_table_type) };

        TableType {
            ty: unsafe { as_c::valtype_to_type(table_type) },
            minimum: unsafe { (*table_limits).min },
            maximum: unsafe {
                if (*table_limits).max == 0 {
                    None
                } else {
                    Some((*table_limits).max)
                }
            },
        }
    }

    pub fn get(&self, store: &mut impl AsStoreMut, index: u32) -> Option<Value> {
        unimplemented!();
        // if let Some(func) = self.handle.table.get(index).ok() {
        //     let ty = FunctionType::new(vec![], vec![]);
        //     let vm_function = VMFunction::new(func, ty);
        //     let function = crate::Function::from_vm_extern(store, vm_function);
        //     Some(Value::FuncRef(Some(function)))
        // } else {
        //     None
        // }
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        unimplemented!();
        // let item = get_function(store, val)?;
        // set_table_item(&self.handle, index, &item)
    }

    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        unimplemented!();
        // let store_mut = store.as_store_ref();
        // let engine = store_mut.engine();
        // let context = engine.0.context();
        // self.handle
        //     .table
        //     .get_property(&context, "length".to_string())
        //     .to_number(&context)
        //     .unwrap() as _
    }

    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        _init: Value,
    ) -> Result<u32, RuntimeError> {
        unimplemented!();
        // let store_mut = store.as_store_mut();
        // let engine = store_mut.engine();
        // let context = engine.0.context();
        // let func = self
        //     .handle
        //     .table
        //     .get_property(&context, "grow".to_string())
        //     .to_object(&context)
        //     .unwrap();
        // match func.call(
        //     &context,
        //     Some(&self.handle.table),
        //     &[JSValue::number(&context, delta as _)],
        // ) {
        //     Ok(val) => Ok(val.to_number(&context).unwrap() as _),
        //     Err(e) => Err(<JSValue as Into<RuntimeError>>::into(e)),
        // }
    }

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

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, vm_extern: VMExternTable) -> Self {
        Self { handle: vm_extern }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
