use std::ptr;

use crate::as_c::{self, param_from_c, result_to_value, type_to_c, valtype_to_type};
use crate::bindings::{
    wasm_frame_copy, wasm_global_get, wasm_global_new, wasm_global_set, wasm_global_type,
    wasm_globaltype_content, wasm_globaltype_mutability, wasm_globaltype_new,
    wasm_mutability_enum_WASM_CONST, wasm_mutability_enum_WASM_VAR, wasm_mutability_t, wasm_val_t,
    wasm_val_t__bindgen_ty_1, wasm_valtype_new,
};
use crate::c_api::bindings::wasm_global_as_extern;
use crate::c_api::vm::{VMExtern, VMGlobal};
use crate::errors::RuntimeError;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::value::Value;
use crate::GlobalType;
use crate::Mutability;
use wasmer_types::{RawValue, Type};

#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub(crate) handle: VMGlobal,
}

unsafe impl Send for Global {}
unsafe impl Sync for Global {}

// Global can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Global {}

impl Global {
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        let extern_ = unsafe { wasm_global_as_extern(self.handle) };
        assert!(
            !extern_.is_null(),
            "Returned null Global extern from wasm-c-api"
        );
        extern_
    }

    /// Create a `Global` with the initial value [`Value`] and the provided [`Mutability`].
    pub(crate) fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        let store = store.as_store_mut();

        let wamr_value = result_to_value(&val);
        let wamr_type = type_to_c(&val.ty());
        let wamr_mutability = if mutability.is_mutable() {
            wasm_mutability_enum_WASM_VAR
        } else {
            wasm_mutability_enum_WASM_CONST
        } as wasm_mutability_t;

        let wamr_global_type =
            unsafe { wasm_globaltype_new(wasm_valtype_new(wamr_type), wamr_mutability) };

        Ok(Self {
            handle: unsafe {
                wasm_global_new(store.inner.store.inner, wamr_global_type, &wamr_value)
            },
        })
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> GlobalType {
        let r#type = unsafe { wasm_global_type(self.handle) };
        let mutability = unsafe { wasm_globaltype_mutability(&*r#type) };
        let valtype = unsafe { wasm_globaltype_content(r#type) };
        let wasmer_type = valtype_to_type(valtype);

        GlobalType::new(
            wasmer_type,
            if mutability == wasm_mutability_enum_WASM_VAR as u8 {
                Mutability::Var
            } else {
                Mutability::Const
            },
        )
    }

    pub fn get(&self, store: &mut impl AsStoreMut) -> Value {
        let mut out = unsafe { std::mem::zeroed() };
        unsafe { wasm_global_get(self.handle, &mut out) };
        param_from_c(&out)
    }

    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        if val.ty() != self.ty(store).ty {
            return Err(RuntimeError::new(format!(
                "Incompatible types: {} != {}",
                val.ty(),
                self.ty(store)
            )));
        }

        if self.ty(store).mutability == Mutability::Const {
            return Err(RuntimeError::new("The global is immutable".to_owned()));
        }

        let value = result_to_value(&val);

        unsafe { wasm_global_set(self.handle, &value) };

        Ok(())
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_global: VMGlobal) -> Self {
        Self { handle: vm_global }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
