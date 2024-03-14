use crate::bindings::{
    wasm_table_type, wasm_tabletype_element, wasm_tabletype_limits, wasm_tabletype_t,
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

// Table can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Table {}

impl Table {
    pub fn new(
        store: &mut impl AsStoreMut,
        ty: TableType,
        init: Value,
    ) -> Result<Self, RuntimeError> {
        unimplemented!();
        // let store_mut = store.as_store_mut();
        // let engine = store_mut.engine();
        // let context = engine.0.context();

        // let mut descriptor = JSObject::new(&context);
        // descriptor.set_property(
        //     &context,
        //     "initial".to_string(),
        //     JSValue::number(&context, ty.minimum.into()),
        // );
        // if let Some(max) = ty.maximum {
        //     descriptor.set_property(
        //         &context,
        //         "maximum".to_string(),
        //         JSValue::number(&context, max.into()),
        //     );
        // }
        // descriptor.set_property(
        //     &context,
        //     "element".to_string(),
        //     JSValue::string(&context, "anyfunc".to_string()),
        // );

        // let js_table = engine
        //     .0
        //     .wasm_table_type()
        //     .construct(&context, &[descriptor.to_jsvalue()])
        //     .map_err(|e| <JSValue as Into<RuntimeError>>::into(e))?;
        // let vm_table = VMTable::new(js_table, ty);
        // Ok(Self::from_vm_extern(store, vm_table))
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
            maximum: unsafe { Some((*table_limits).max) },
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
