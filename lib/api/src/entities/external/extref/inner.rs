use std::any::Any;

use crate::StoreRef;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::macros::backend::{gen_rt_ty, match_rt};
use crate::vm::VMExternRef;

gen_rt_ty! {
    #[derive(derive_more::From, Debug, Clone)]
    pub(crate) BackendExternRef(entities::external::ExternRef);
}

impl BackendExternRef {
    /// Make a new extern reference
    #[inline]
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Self::Sys(
                crate::backend::sys::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Self::V8(
                crate::backend::v8::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Self::Js(
                crate::backend::js::entities::external::ExternRef::new(store, value),
            ),
        }
    }

    /// Try to downcast to the given value.
    #[inline]
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        match_rt!(on self => r {
            r.downcast::<T>(store)
        })
    }

    /// Create a [`VMExternRef`] from [`Self`].
    #[inline]
    pub(crate) fn vm_externref(&self) -> VMExternRef {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => VMExternRef::Sys(r.vm_externref()),
            #[cfg(feature = "v8")]
            Self::V8(r) => VMExternRef::V8(r.vm_externref()),
            #[cfg(feature = "js")]
            Self::Js(r) => VMExternRef::Js(r.vm_externref()),
        }
    }

    /// Create an instance of [`Self`] from a [`VMExternRef`].
    #[inline]
    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(unsafe {
                crate::backend::sys::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.unwrap_sys(),
                )
            }),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(unsafe {
                crate::backend::v8::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.unwrap_v_8(),
                )
            }),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(unsafe {
                crate::backend::js::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.unwrap_js(),
                )
            }),
        }
    }

    /// Checks whether this `ExternRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => r {
            r.is_from_store(store)
        })
    }
}
