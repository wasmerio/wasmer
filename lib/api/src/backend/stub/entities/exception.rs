use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExceptionRef;
use crate::{ExceptionHandle, RuntimeError, Value};
use wasmer_types::TagType;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exception;

impl Exception {
    pub fn new(
        _store: &mut impl AsStoreMut,
        _tag: crate::Tag,
        _payload: &[Value],
    ) -> Self {
        panic!("stub backend cannot create exceptions")
    }

    pub fn ty(&self) -> TagType {
        panic!("stub backend does not expose exception types")
    }

    pub fn payload(&self) -> &[Value] {
        panic!("stub backend does not expose exception payloads")
    }

    pub fn into_handle(self) -> ExceptionHandle {
        panic!("stub backend cannot expose exception handles")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExceptionRef;

impl ExceptionRef {
    pub fn new<T>(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
    {
        panic!("stub backend cannot create exception refs")
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: std::any::Any + Send + Sync + 'static + Sized,
    {
        panic!("stub backend cannot downcast exception refs")
    }

    pub fn vm_exceptionref(&self) -> VMExceptionRef {
        panic!("stub backend cannot expose VM exception refs")
    }

    pub unsafe fn from_vm_exceptionref(
        _store: &mut impl AsStoreMut,
        _vm_exceptionref: VMExceptionRef,
    ) -> Self {
        panic!("stub backend cannot import exception refs")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic!("stub backend cannot verify exception ref origins")
    }
}
