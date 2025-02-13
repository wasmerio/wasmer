//! Data types, functions and traits for `wasmi`'s `ExternRef` implementation.
use crate::{
    store::{AsStoreMut, AsStoreRef},
    wasmi::vm::VMExternRef,
};
use std::any::Any;

#[derive(Debug, Clone)]
#[repr(transparent)]
/// A WebAssembly `extern ref` in `wasmi`.
pub struct ExternRef;

impl ExternRef {
    pub fn new<T>(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!("ExternRef is not yet supported with wasm_c_api");
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!("ExternRef is not yet supported in wasm_c_api");
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        unimplemented!("ExternRef is not yet supported in wasm_c_api");
    }

    pub(crate) unsafe fn from_vm_externref(
        _store: &mut impl AsStoreMut,
        _vm_externref: VMExternRef,
    ) -> Self {
        unimplemented!("ExternRef is not yet supported in wasm_c_api");
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
