//! Data types, functions and traits for `sys` runtime's `Tag` implementation.
use std::any::Any;

use wasmer_types::{TagType, Type};
use wasmer_vm::StoreHandle;

use crate::{
    sys::vm::{VMException, VMExceptionRef},
    AsStoreMut, AsStoreRef, Tag, Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `exception` in the `sys` runtime.
pub(crate) struct Exception {
    pub(crate) handle: VMException,
}

unsafe impl Send for Exception {}
unsafe impl Sync for Exception {}

impl Exception {
    /// Create a new [`Exception`].
    pub fn new(store: &mut impl AsStoreMut, tag: Tag, payload: &[Value]) -> Self {
        todo!()
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
/// A WebAssembly `extern ref` in the `sys` runtime.
pub(crate) struct ExceptionRef {
    handle: StoreHandle<wasmer_vm::VMExceptionObj>,
}

impl ExceptionRef {
    /// Make a new extern reference
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self {
            handle: crate::backend::sys::store::StoreHandle::new(
                store.objects_mut().as_sys_mut(),
                wasmer_vm::VMExceptionObj::new(value),
            ),
        }
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .as_ref()
            .downcast_ref::<T>()
    }

    /// Create a [`VMExceptionRef`] from [`Self`].
    pub(crate) fn vm_exceptionref(&self) -> VMExceptionRef {
        wasmer_vm::VMExceptionRef(self.handle.internal_handle())
    }

    /// Create an instance of [`Self`] from a [`VMExceptionRef`].
    pub(crate) unsafe fn from_vm_exceptionref(
        store: &mut impl AsStoreMut,
        vm_exceptionref: VMExceptionRef,
    ) -> Self {
        Self {
            handle: StoreHandle::from_internal(store.objects_mut().id(), vm_exceptionref.0),
        }
    }

    /// Checks whether this `ExceptionRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/exceptionref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// exceptionref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }
}

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for Exception {
    fn size_of_val(&self, tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of::<Self>()
    }
}
