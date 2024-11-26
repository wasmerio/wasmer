use crate::as_c::{param_from_c, result_to_value};
use crate::bindings::{
    wasm_extern_as_ref, wasm_func_as_ref, wasm_limits_t, wasm_ref_as_func, wasm_ref_as_trap,
    wasm_ref_t, wasm_table_copy, wasm_table_get, wasm_table_grow, wasm_table_new, wasm_table_set,
    wasm_table_size, wasm_table_type, wasm_tabletype_element, wasm_tabletype_limits,
    wasm_tabletype_new, wasm_tabletype_t, wasm_val_t, wasm_valkind_enum_WASM_FUNCREF,
    wasm_valtype_new,
};

#[cfg(not(feature = "v8"))]
use crate::bindings::wasm_valkind_enum_WASM_EXTERNREF;

#[cfg(feature = "v8")]
use crate::bindings::wasm_valkind_enum_WASM_ANYREF as wasm_valkind_enum_WASM_EXTERNREF;

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
        unsafe {
            let ref_ = wasm_table_get(self.handle, index);

            if ref_.is_null() {
                return None;
            }

            let kind = match self.ty(store).ty {
                wasmer_types::Type::ExternRef => wasm_valkind_enum_WASM_EXTERNREF,
                wasmer_types::Type::FuncRef => wasm_valkind_enum_WASM_FUNCREF,
                ty => panic!("unsupported table type: {ty:?}"),
            } as u8;

            let value = {
                #[cfg(feature = "wamr")]
                {
                    wasm_val_t {
                        kind,
                        _paddings: Default::default(),
                        of: crate::bindings::wasm_val_t__bindgen_ty_1 { ref_ },
                    }
                }
                #[cfg(not(feature = "wamr"))]
                {
                    wasm_val_t {
                        kind,
                        of: crate::bindings::wasm_val_t__bindgen_ty_1 { ref_ },
                    }
                }
            };

            Some(param_from_c(&value))
        }
    }

    pub fn set(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
        val: Value,
    ) -> Result<(), RuntimeError> {
        unsafe {
            let init = match val {
                Value::ExternRef(None) | Value::FuncRef(None) => std::ptr::null_mut(),
                Value::FuncRef(Some(ref r)) => wasm_func_as_ref(r.0.handle),
                _ => {
                    return Err(RuntimeError::new(format!(
                        "Could not grow table due to unsupported init value type: {val:?} "
                    )))
                }
            };

            if !wasm_table_set(self.handle, index, init) {
                return Err(RuntimeError::new(format!(
                    "Could not set value {val:?} table at index {index}"
                )));
            }

            Ok(())
        }
    }

    pub fn size(&self, store: &impl AsStoreRef) -> u32 {
        unsafe { wasm_table_size(self.handle) }
    }

    pub fn grow(
        &self,
        store: &mut impl AsStoreMut,
        delta: u32,
        init: Value,
    ) -> Result<u32, RuntimeError> {
        unsafe {
            let size = wasm_table_size(self.handle);
            let init = match init {
                Value::ExternRef(None) | Value::FuncRef(None) => std::ptr::null_mut(),
                Value::FuncRef(Some(r)) => wasm_func_as_ref(r.0.handle),
                _ => {
                    return Err(RuntimeError::new(format!(
                        "Could not grow table due to unsupported init value type: {init:?} "
                    )))
                }
            };
            if !wasm_table_grow(self.handle, delta, init) {
                return Err(RuntimeError::new("Could not grow table"));
            }

            Ok(size)
        }
    }

    pub fn copy(
        _store: &mut impl AsStoreMut,
        _dst_table: &Self,
        _dst_index: u32,
        _src_table: &Self,
        _src_index: u32,
        _len: u32,
    ) -> Result<(), RuntimeError> {
        unimplemented!("Copying tables is currently not implemented!")
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, vm_extern: VMExternTable) -> Self {
        Self { handle: vm_extern }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
