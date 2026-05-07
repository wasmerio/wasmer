//! Data types, functions and traits for `v8` runtime's `ExternRef` implementation.
use crate::{
    store::{AsStoreMut, AsStoreRef},
    v8::{
        bindings::*,
        vm::VMExternRef,
    },
};
use std::{any::Any, ffi::c_void, ptr::NonNull};

struct ExternRefHostData {
    value: Box<dyn Any + Send + Sync + 'static>,
}

unsafe extern "C" fn externref_host_info_finalizer(info: *mut c_void) {
    if let Some(info) = NonNull::new(info as *mut ExternRefHostData) {
        unsafe {
            drop(Box::from_raw(info.as_ptr()));
        }
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
/// A WebAssembly `extern ref` in the `v8` runtime.
pub struct ExternRef {
    handle: VMExternRef,
}

impl ExternRef {
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        let mut store = store.as_store_mut();
        let v8_store = store.inner.store.as_v8();

        let foreign = unsafe { wasm_foreign_new(v8_store.inner) };
        assert!(!foreign.is_null(), "failed to create v8 externref");

        let data = Box::into_raw(Box::new(ExternRefHostData {
            value: Box::new(value),
        }));

        unsafe {
            wasm_foreign_set_host_info_with_finalizer(
                foreign,
                data as *mut c_void,
                Some(externref_host_info_finalizer),
            );
        }

        Self {
            handle: VMExternRef::from_owned_raw(unsafe { wasm_foreign_as_ref(foreign) }),
        }
    }

    pub fn downcast<'a, T>(&self, _store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        let foreign = unsafe { wasm_ref_as_foreign(self.handle.as_raw()) };
        if foreign.is_null() {
            return None;
        }

        let info = unsafe { wasm_foreign_get_host_info(foreign) };
        let info = NonNull::new(info as *mut ExternRefHostData)?;
        unsafe { (*info.as_ptr()).value.downcast_ref::<T>() }
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        self.handle.clone()
    }

    pub(crate) unsafe fn from_vm_externref(
        _store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        Self {
            handle: vm_externref,
        }
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl From<VMExternRef> for ExternRef {
    fn from(handle: VMExternRef) -> Self {
        Self { handle }
    }
}
