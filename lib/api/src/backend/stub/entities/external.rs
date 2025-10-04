use std::any::Any;

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
        Self
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        None
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        VMExternRef::stub()
    }

    pub(crate) unsafe fn from_vm_externref(
        _store: &mut impl AsStoreMut,
        _vm_externref: VMExternRef,
    ) -> Self {
        Self
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}
