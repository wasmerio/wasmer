use std::any::Any;

use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternRef;
use crate::StoreRef;

#[derive(Debug)]
#[repr(transparent)]
/// An opaque reference to some data. This reference can be passed through Wasm.
pub struct ExternRef(pub(crate) Box<dyn ExternRefLike>);

impl ExternRef {
    /// Make a new extern reference
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self(store.as_store_mut().extern_ref_new(value))
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        store.as_store_ref().downcast_extern_ref(self.0.as_ref())
    }

    /// Create a [`VMExternRef`] from [`Self`].
    pub(crate) fn vm_externref(&self) -> VMExternRef {
        self.0.vm_externref()
    }

    /// Create an instance of [`Self`] from a [`VMExternRef`].
    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        Self(store.as_store_mut().extern_ref_from_vm(vm_externref))
    }

    /// Checks whether this `ExternRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(&store.as_store_ref())
    }
}

impl Clone for ExternRef {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

/// The trait that every concrete extern reference must implement.
pub trait ExternRefLike: std::fmt::Debug {
    /// Create a [`VMExternRef`] from an [`ExternRefLike`].
    fn vm_externref(&self) -> VMExternRef;

    /// Checks whether this `ExternRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    fn is_from_store(&self, store: &StoreRef) -> bool;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn ExternRefLike>;
}

/// The trait implemented by all those that can create new extern references.
pub trait ExternRefCreator {
    /// Make a new extern reference
    fn extern_ref_new<T>(&mut self, value: T) -> Box<dyn ExternRefLike>
    where
        T: Any + Send + Sync + 'static + Sized,
        Self: Sized;

    /// Create an instance of [`ExternRefLike`] from a [`VMExternRef`].
    unsafe fn extern_ref_from_vm(&mut self, vm_externref: VMExternRef) -> Box<dyn ExternRefLike>;
}

/// The trait implemented by all those that can inspect existing references.
pub trait ExternRefResolver {
    /// Try to downcast an [`ExternRefLike`] into the concrete type `T`.
    fn downcast_extern_ref<'a, T>(&self, extref: &dyn ExternRefLike) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
        Self: Sized;
}
