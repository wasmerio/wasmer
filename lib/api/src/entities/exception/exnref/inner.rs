use std::any::Any;

use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::macros::backend::{gen_rt_ty, match_rt};
use crate::vm::VMExceptionRef;
use crate::StoreRef;

gen_rt_ty!(ExceptionRef @derives derive_more::From, Debug, Clone ; @path exception);

impl BackendExceptionRef {
    /// Make a new extern reference
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Self::Sys(
                crate::backend::sys::entities::exception::ExceptionRef::new(store, value),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => Self::Wamr(
                crate::backend::wamr::entities::exception::ExceptionRef::new(store, value),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => Self::Wasmi(
                crate::backend::wasmi::entities::exception::ExceptionRef::new(store, value),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Self::V8(
                crate::backend::v8::entities::exception::ExceptionRef::new(store, value),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Self::Js(
                crate::backend::js::entities::exception::ExceptionRef::new(store, value),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => Self::Jsc(
                crate::backend::jsc::entities::exception::ExceptionRef::new(store, value),
            ),
        }
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        match_rt!(on self => r {
            r.downcast::<T>(store)
        })
    }

    /// Create a [`VMExceptionRef`] from [`Self`].
    pub(crate) fn vm_exceptionref(&self) -> VMExceptionRef {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => VMExceptionRef::Sys(r.vm_exceptionref()),
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => VMExceptionRef::Wamr(r.vm_exceptionref()),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(r) => VMExceptionRef::Wasmi(r.vm_exceptionref()),
            #[cfg(feature = "v8")]
            Self::V8(r) => VMExceptionRef::V8(r.vm_exceptionref()),
            #[cfg(feature = "js")]
            Self::Js(r) => VMExceptionRef::Js(r.vm_exceptionref()),
            #[cfg(feature = "jsc")]
            Self::Jsc(r) => VMExceptionRef::Jsc(r.vm_exceptionref()),
        }
    }

    /// Create an instance of [`Self`] from a [`VMExceptionRef`].
    pub(crate) unsafe fn from_vm_exceptionref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExceptionRef,
    ) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_sys(),
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_wamr(),
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_wasmi(),
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_v8(),
                ),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_js(),
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::exception::ExceptionRef::from_vm_exceptionref(
                    store,
                    vm_externref.into_jsc(),
                ),
            ),
        }
    }

    /// Checks whether this `ExceptionRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => r {
            r.is_from_store(store)
        })
    }
}
