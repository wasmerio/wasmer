use std::any::Any;

use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::{VMExceptionRef, VMExternRef};
use crate::StoreRef;

pub(crate) mod inner;
pub(crate) use inner::*;

#[derive(Debug, Clone, derive_more::From)]
/// An opaque reference to some data. This reference can be passed through Wasm.
pub struct ExceptionRef(pub(crate) BackendExceptionRef);

impl ExceptionRef {
    /// Make a new extern reference
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self(BackendExceptionRef::new(store, value))
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        self.0.downcast(store)
    }

    /// Create a [`VMExceptionRef`] from [`Self`].
    pub(crate) fn vm_exceptionref(&self) -> VMExceptionRef {
        self.0.vm_exceptionref()
    }

    /// Create an instance of [`Self`] from a [`VMExceptionRef`].
    pub(crate) unsafe fn from_vm_exceptionref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExceptionRef,
    ) -> Self {
        Self(BackendExceptionRef::from_vm_exceptionref(
            store,
            vm_externref,
        ))
    }

    /// Checks whether this `ExceptionRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }
}
