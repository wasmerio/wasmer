use std::any::Any;

use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternRef;

/// Minimal extern reference for the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternRef;

impl ExternRef {
    pub fn new<T>(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        panic_stub("cannot create extern refs")
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        panic_stub("cannot downcast extern refs")
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        VMExternRef::Stub(crate::backend::stub::vm::VMExternRef::stub())
    }

    pub(crate) unsafe fn from_vm_externref(
        _store: &mut impl AsStoreMut,
        _vm_externref: VMExternRef,
    ) -> Self {
        panic_stub("cannot import extern refs")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify extern ref origins")
    }
}
