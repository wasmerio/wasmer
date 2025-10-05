use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExceptionRef;
use crate::Value;
use wasmer_types::TagType;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exception;

impl Exception {
    pub fn new(_store: &mut impl AsStoreMut, _tag: crate::Tag, _payload: &[Value]) -> Self {
        panic_stub("cannot create exceptions")
    }

    pub fn ty(&self) -> TagType {
        panic_stub("does not expose exception types")
    }

    pub fn payload(&self) -> &[Value] {
        panic_stub("does not expose exception payloads")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExceptionRef;

impl ExceptionRef {
    pub fn new<T>(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
    {
        panic_stub("cannot create exception refs")
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
    {
        panic_stub("cannot downcast exception refs")
    }

    pub fn vm_exceptionref(&self) -> VMExceptionRef {
        panic_stub("cannot expose VM exception refs")
    }

    pub unsafe fn from_vm_exceptionref(
        _store: &mut impl AsStoreMut,
        _vm_exceptionref: VMExceptionRef,
    ) -> Self {
        panic_stub("cannot import exception refs")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify exception ref origins")
    }
}
