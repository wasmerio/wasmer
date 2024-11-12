use std::any::Any;
use wasmer_vm::VMExternRef;
use wasmer_vm::{StoreHandle, VMExternObj};

use super::store::Store;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::ExternRefLike;

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct ExternRef {}

impl ExternRef {
    pub fn new<T>(store: &mut Store, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        todo!()
    }

    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        todo!()
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        todo!()
    }

    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        todo!()
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        todo!()
    }
}

impl ExternRefLike for ExternRef {
    fn vm_externref(&self) -> crate::vm::VMExternRef {
        todo!()
    }

    fn is_from_store(&self, store: &crate::StoreRef) -> bool {
        todo!()
    }

    fn clone_box(&self) -> Box<dyn ExternRefLike> {
        todo!()
    }
}
