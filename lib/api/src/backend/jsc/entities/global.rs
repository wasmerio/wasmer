use rusty_jsc::{JSObject, JSValue};
use wasmer_types::{GlobalType, Mutability, Type};

use crate::{
    jsc::{
        utils::convert::{jsc_value_to_wasmer, AsJsc},
        vm::VMGlobal,
    },
    vm::VMExtern,
    AsStoreMut, AsStoreRef, BackendGlobal, RuntimeError, Value,
};

use super::store::StoreObject;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    pub(crate) handle: VMGlobal,
}

// Global can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Global {}

impl Global {
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Jsc(crate::backend::jsc::vm::VMExtern::Global(
            self.handle.clone(),
        ))
    }

    /// Create a `Global` with the initial value [`Value`] and the provided [`Mutability`].
    pub(crate) fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        let global_ty = GlobalType {
            mutability,
            ty: val.ty(),
        };
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();

        let mut descriptor = JSObject::new(&context);
        let type_str = match val.ty() {
            Type::I32 => "i32",
            Type::I64 => "i64",
            Type::F32 => "f32",
            Type::F64 => "f64",
            ty => unimplemented!(
                "The type: {:?} is not yet supported in the JS Global API",
                ty
            ),
        };
        // This is the value type as string, even though is incorrectly called "value"
        // in the JS API.
        descriptor.set_property(
            &context,
            "value".to_string(),
            JSValue::string(&context, type_str.to_string()),
        );
        descriptor.set_property(
            &context,
            "mutable".to_string(),
            JSValue::boolean(&context, mutability.is_mutable()),
        );

        let value: JSValue = val.as_jsc_value(&store_mut);
        let js_global = engine
            .as_jsc()
            .wasm_global_type()
            .construct(&context, &[descriptor.to_jsvalue(), value])
            .map_err(|e| <JSValue as Into<RuntimeError>>::into(e))?;
        let vm_global = VMGlobal::new(js_global, global_ty);
        crate::backend::jsc::vm::VMGlobal::list_mut(store.objects_mut().as_jsc_mut())
            .push(vm_global.clone());
        Ok(Self { handle: vm_global })
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> GlobalType {
        self.handle.ty
    }

    pub fn get(&self, store: &mut impl AsStoreMut) -> Value {
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        let value = self
            .handle
            .global
            .get_property(&context, "value".to_string());
        jsc_value_to_wasmer(&context, &self.handle.ty.ty, &value)
    }

    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        let store_mut = store.as_store_mut();
        let new_value = val.as_jsc_value(&store_mut);
        let engine = store_mut.engine();
        let context = engine.as_jsc().context();
        self.handle
            .global
            .set_property(&context, "value".to_string(), new_value)
            .map_err(|e| <JSValue as Into<RuntimeError>>::into(e))
    }

    pub(crate) fn from_vm_extern(
        store: &mut impl AsStoreMut,
        vm_global: crate::vm::VMExternGlobal,
    ) -> Self {
        crate::backend::jsc::vm::VMGlobal::list_mut(store.objects_mut().as_jsc_mut())
            .push(vm_global.as_jsc().clone());
        Self {
            handle: vm_global.into_jsc(),
        }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl crate::Global {
    /// Consume [`self`] into [`crate::backend::jsc::global::Global`].
    pub fn into_jsc(self) -> crate::backend::jsc::global::Global {
        match self.0 {
            BackendGlobal::Jsc(s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::global::Global`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::global::Global {
        match self.0 {
            BackendGlobal::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::global::Global`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::global::Global {
        match self.0 {
            BackendGlobal::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` global!"),
        }
    }
}
