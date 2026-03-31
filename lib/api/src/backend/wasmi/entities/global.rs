//! Data types, functions and traits for `wasmi`'s `Global` implementation.
use ::wasmi as wasmi_native;
use wasmer_types::{GlobalType, Mutability};

use crate::{
    AsStoreMut, AsStoreRef, RuntimeError, Value,
    vm::{VMExtern, VMExternGlobal},
    wasmi::{
        utils::convert::{IntoCApiType, IntoCApiValue, IntoWasmerType, IntoWasmerValue},
        vm::{handle_bits, VMGlobal},
    },
};

#[derive(Debug, Clone)]
/// A WebAssembly `global` in `wasmi`.
pub struct Global {
    pub(crate) handle: VMGlobal,
}

unsafe impl Send for Global {}
unsafe impl Sync for Global {}

impl PartialEq for Global {
    fn eq(&self, other: &Self) -> bool {
        handle_bits(self.handle) == handle_bits(other.handle)
    }
}

impl Eq for Global {}

impl Global {
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Wasmi(crate::backend::wasmi::vm::VMExtern::Global(self.handle))
    }

    pub(crate) fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        let mut store = store.as_store_mut();
        Ok(Self {
            handle: wasmi_native::Global::new(
                &mut store.inner.store.as_wasmi_mut().inner,
                val.into_cv(),
                if mutability.is_mutable() {
                    wasmi_native::Mutability::Var
                } else {
                    wasmi_native::Mutability::Const
                },
            ),
        })
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> GlobalType {
        let ty = self
            .handle
            .ty(&store.as_store_ref().inner.store.as_wasmi().inner);
        GlobalType::new(
            ty.content().into_wt(),
            if ty.mutability().is_mut() {
                Mutability::Var
            } else {
                Mutability::Const
            },
        )
    }

    pub fn get(&self, store: &mut impl AsStoreMut) -> Value {
        self.handle
            .get(&store.as_store_ref().inner.store.as_wasmi().inner)
            .into_wv()
    }

    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        self.handle
            .set(&mut store.as_store_mut().inner.store.as_wasmi_mut().inner, val.into_cv())
            .map_err(|err| RuntimeError::new(err.to_string()))
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, vm_global: VMExternGlobal) -> Self {
        let crate::vm::VMExternGlobal::Wasmi(handle) = vm_global else {
            panic!("Not a `wasmi` global extern")
        };
        Self { handle }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
