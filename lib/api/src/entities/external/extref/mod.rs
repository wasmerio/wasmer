use std::any::Any;

use crate::StoreRef;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMExternRef;

pub(crate) mod inner;
pub(crate) use inner::*;

#[derive(Debug, Clone, derive_more::From)]
/// An opaque reference to some data. This reference can be passed through Wasm.
pub struct ExternRef(pub(crate) BackendExternRef);

impl ExternRef {
    /// Make a new extern reference
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self(BackendExternRef::new(store, value))
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        self.0.downcast(store)
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
        Self(unsafe { BackendExternRef::from_vm_externref(store, vm_externref) })
    }

    /// Checks whether this `ExternRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Whether two [`ExternRef`]s refer to the same underlying extern object.
    pub fn ptr_eq(&self, other: &ExternRef) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ExternRef, Store};

    #[test]
    fn ptr_eq_tracks_object_identity() {
        let mut store = Store::default();

        let a = ExternRef::new(&mut store, 7i32);
        // A clone is the same underlying extern object.
        assert!(a.ptr_eq(&a.clone()));

        // A separately created ref (even with an equal payload) is distinct.
        let b = ExternRef::new(&mut store, 7i32);
        assert!(!a.ptr_eq(&b));
        assert!(!b.ptr_eq(&a));

        // The downcast payloads are what we expect.
        assert_eq!(a.downcast::<i32>(&store), Some(&7));
        assert_eq!(b.downcast::<i32>(&store), Some(&7));
    }
}
