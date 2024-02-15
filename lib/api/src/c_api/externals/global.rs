use std::ptr;

use crate::as_c::{param_from_c, result_to_value, type_to_c, valtype_to_type};
use crate::bindings::{
    wasm_global_get, wasm_global_new, wasm_global_set, wasm_global_type, wasm_globaltype_content,
    wasm_globaltype_mutability, wasm_globaltype_new, wasm_mutability_enum_WASM_CONST,
    wasm_mutability_enum_WASM_VAR, wasm_mutability_t, wasm_val_t, wasm_val_t__bindgen_ty_1,
    wasm_valtype_new,
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
        let c_value = result_to_value(&val);
        let globaltype = unsafe {
            let type_ = type_to_c(&val.ty());
            let mutability: wasm_mutability_t = if mutability.is_mutable() {
                wasm_mutability_enum_WASM_VAR as wasm_mutability_t
            } else {
                wasm_mutability_enum_WASM_CONST as wasm_mutability_t
            };
            let valtype = wasm_valtype_new(type_);
            wasm_globaltype_new(valtype, mutability)
        };
        let handle = unsafe { wasm_global_new(store.inner.store.inner, globaltype, &c_value) };
        Ok(Self { handle })

        // if !val.is_from_store(store) {
        //     return Err(RuntimeError::new("cross-`Store` values are not supported"));
        // }
        // let global_ty = GlobalType {
        //     mutability,
        //     ty: val.ty(),
        // };
        // let store_mut = store.as_store_mut();
        // let engine = store_mut.engine();
        // let context = engine.0.context();

        // let mut descriptor = JSObject::new(&context);
        // let type_str = match val.ty() {
        //     Type::I32 => "i32",
        //     Type::I64 => "i64",
        //     Type::F32 => "f32",
        //     Type::F64 => "f64",
        //     ty => unimplemented!(
        //         "The type: {:?} is not yet supported in the JS Global API",
        //         ty
        //     ),
        // };
        // // This is the value type as string, even though is incorrectly called "value"
        // // in the JS API.
        // descriptor.set_property(
        //     &context,
        //     "value".to_string(),
        //     JSValue::string(&context, type_str.to_string()),
        // );
        // descriptor.set_property(
        //     &context,
        //     "mutable".to_string(),
        //     JSValue::boolean(&context, mutability.is_mutable()),
        // );

        // let value: JSValue = val.as_jsvalue(&store_mut);
        // let js_global = engine
        //     .0
        //     .wasm_global_type()
        //     .construct(&context, &[descriptor.to_jsvalue(), value])
        //     .map_err(|e| <JSValue as Into<RuntimeError>>::into(e))?;
        // let vm_global = VMGlobal::new(js_global, global_ty);
        // Ok(Self::from_vm_extern(store, vm_global))
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> GlobalType {
        let type_ = unsafe { wasm_global_type(self.handle) };
        let mutability = unsafe { wasm_globaltype_mutability(&*type_) };
        let valtype = unsafe { wasm_globaltype_content(type_) };
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
        let mut out = wasm_val_t {
            kind: 0,
            of: wasm_val_t__bindgen_ty_1 { i32_: 0 },
        };
        unsafe { wasm_global_get(self.handle, &mut out) };
        param_from_c(&out)
    }

    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
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
