use std::any::Any;

use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::macros::rt::{gen_rt_ty, match_rt};
use crate::vm::VMExternRef;
use crate::StoreRef;

gen_rt_ty!(ExternRef @derives derive_more::From, Debug, Clone ; @path external);

impl RuntimeExternRef {
    /// Make a new extern reference
    #[inline]
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(s) => Self::Sys(
                crate::rt::sys::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(s) => Self::Wamr(
                crate::rt::wamr::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "wasmi")]
            crate::RuntimeStore::Wasmi(s) => Self::Wasmi(
                crate::rt::wasmi::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(s) => Self::V8(
                crate::rt::v8::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(s) => Self::Js(
                crate::rt::js::entities::external::ExternRef::new(store, value),
            ),
            #[cfg(feature = "jsc")]
            crate::RuntimeStore::Jsc(s) => Self::Jsc(
                crate::rt::jsc::entities::external::ExternRef::new(store, value),
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
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => VMExternRef::Wamr(r.vm_externref()),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(r) => VMExternRef::Wasmi(r.vm_externref()),
            #[cfg(feature = "v8")]
            Self::V8(r) => VMExternRef::V8(r.vm_externref()),
            #[cfg(feature = "js")]
            Self::Js(r) => VMExternRef::Js(r.vm_externref()),
            #[cfg(feature = "jsc")]
            Self::Jsc(r) => VMExternRef::Jsc(r.vm_externref()),
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
            crate::RuntimeStore::Sys(_) => Self::Sys(
                crate::rt::sys::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_sys(),
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => Self::Wamr(
                crate::rt::wamr::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_wamr(),
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::RuntimeStore::Wasmi(_) => Self::Wasmi(
                crate::rt::wasmi::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_wasmi(),
                ),
            ),
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => Self::V8(
                crate::rt::v8::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_v8(),
                ),
            ),
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => Self::Js(
                crate::rt::js::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_js(),
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::RuntimeStore::Jsc(_) => Self::Jsc(
                crate::rt::jsc::entities::external::ExternRef::from_vm_externref(
                    store,
                    vm_externref.into_jsc(),
                ),
            ),
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
