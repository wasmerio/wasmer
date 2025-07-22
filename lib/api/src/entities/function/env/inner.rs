use crate::{
    macros::backend::{gen_rt_ty, match_rt}, AsStoreMut, AsStoreRef, FunctionEnv, FunctionEnvMut,
    StoreMut, StoreRef,
};
use std::{any::Any, marker::PhantomData};

use wasmer_types::Upcast;

gen_rt_ty! {
    /// An opaque reference to a function environment.
    /// The function environment data is owned by the `Store`.
    #[derive(Debug, derive_more::From)]
    pub BackendFunctionEnv<T>(function::env::FunctionEnv<T>);
}

impl<T> Clone for BackendFunctionEnv<T> {
    fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => Self::Sys(s.clone()),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => Self::Wamr(s.clone()),

            #[cfg(feature = "wasmi")]
            Self::Wasmi(s) => Self::Wasmi(s.clone()),
            #[cfg(feature = "v8")]
            Self::V8(s) => Self::V8(s.clone()),
            #[cfg(feature = "js")]
            Self::Js(s) => Self::Js(s.clone()),
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => Self::Jsc(s.clone()),
        }
    }
}

impl<T> BackendFunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut<Object: Upcast<T>>, value: T) -> Self {
        match store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::function::env::FunctionEnv::new(store, value),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::function::env::FunctionEnv::new(store, value),
            ),

            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::function::env::FunctionEnv::new(store, value),
            ),

            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::function::env::FunctionEnv::new(store, value),
            ),

            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::function::env::FunctionEnv::new(store, value),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::function::env::FunctionEnv::new(store, value),
            ),
        }
    }

    //#[allow(dead_code)] // This function is only used in js
    //pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
    //    todo!()
    //}

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef<Object: Upcast<T>>) -> &'a T {
        match_rt!(on self => f {
            f.as_ref(store)
        })
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut<Object: Upcast<T>>) -> &'a mut T {
        match_rt!(on self => s {
            s.as_mut(store)
        })
    }

    /// Convert it into a `FunctionEnvMut`
    // TODO consider taking the `AsStoreMut` directly
    pub fn into_mut<S: AsStoreMut>(self, store: &mut S) -> FunctionEnvMut<'_, T, S::Object>
    where
        T: Any + Send + 'static + Sized,
    {
        match_rt!(on self => f {
            f.into_mut(store).into()
        })
    }
}

gen_rt_ty! {
    /// A temporary handle to a [`FunctionEnv`].
    #[derive(derive_more::From)]
    pub BackendFunctionEnvMut<'a, T, Object>(function::env::FunctionEnvMut<'a, T, Object>);
}

impl<T, Object: Upcast<T>> BackendFunctionEnvMut<'_, T, Object> {
    /// Returns a reference to the host state in this function environement.
    pub fn data(&self) -> &T {
        match_rt!(on self => f {
            f.data()
        })
    }

    /// Returns a mutable- reference to the host state in this function environement.
    pub fn data_mut(&mut self) -> &mut T {
        match_rt!(on self => f {
            f.data_mut()
        })
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref f) => BackendFunctionEnv::Sys(f.as_ref()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref f) => BackendFunctionEnv::Wamr(f.as_ref()).into(),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(ref f) => BackendFunctionEnv::Wasmi(f.as_ref()).into(),
            #[cfg(feature = "v8")]
            Self::V8(ref f) => BackendFunctionEnv::V8(f.as_ref()).into(),
            #[cfg(feature = "js")]
            Self::Js(ref f) => BackendFunctionEnv::Js(f.as_ref()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(ref f) => BackendFunctionEnv::Jsc(f.as_ref()).into(),
        }
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T, Object> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut f) => BackendFunctionEnvMut::Sys(f.as_mut()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut f) => BackendFunctionEnvMut::Wamr(f.as_mut()).into(),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(ref mut f) => BackendFunctionEnvMut::Wasmi(f.as_mut()).into(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut f) => BackendFunctionEnvMut::V8(f.as_mut()).into(),
            #[cfg(feature = "js")]
            Self::Js(ref mut f) => BackendFunctionEnvMut::Js(f.as_mut()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(ref mut f) => BackendFunctionEnvMut::Jsc(f.as_mut()).into(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_, Object>) {
        match_rt!(on self => f {
            f.data_and_store_mut()
        })
    }
}

impl<T, Object> AsStoreRef for BackendFunctionEnvMut<'_, T, Object> {
    type Object = Object;

    fn as_store_ref(&self) -> StoreRef<'_, Object> {
        match_rt!(on &self => f {
            f.as_store_ref()
        })
    }
}

impl<T, Object> AsStoreMut for BackendFunctionEnvMut<'_, T, Object> {
    fn as_store_mut(&mut self) -> StoreMut<'_, Object> {
        match_rt!(on self => s {
            s.as_store_mut()
        })
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects<Object> {
        match_rt!(on self => s {
            s.objects_mut()
        })
    }
}

impl<T: std::fmt::Debug, Object: Upcast<T>> std::fmt::Debug for BackendFunctionEnvMut<'_, T, Object> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match_rt!(on self => s {
            write!(f, "{s:?}")
        })
    }
}
