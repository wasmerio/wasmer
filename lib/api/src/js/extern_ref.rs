use std::any::Any;

use crate::store::{AsStoreMut, AsStoreRef};
use wasmer_types::RawValue;

pub(crate) struct VMExternRef;

impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!();
    }

    /// Extracts a `VMExternRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExternRef` instance.
    pub unsafe fn from_raw(raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct ExternRef;

impl ExternRef {
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!();
    }

    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        unimplemented!();
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        unimplemented!();
    }

    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        unimplemented!();
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        true
    }
}
