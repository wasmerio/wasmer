use std::any::Any;

use crate::js::vm::VMExternRef;
use crate::store::{AsStoreMut, AsStoreRef};
use wasmer_types::RawValue;

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct ExternRef;

impl ExternRef {
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!("ExternRef is not yet supported in Javascript");
    }

    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!("ExternRef is not yet supported in Javascript");
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        unimplemented!("ExternRef is not yet supported in Javascript");
    }

    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        unimplemented!("ExternRef is not yet supported in Javascript");
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        true
    }
}
