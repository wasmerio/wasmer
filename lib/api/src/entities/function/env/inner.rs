use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, FunctionEnv, FunctionEnvMut, StoreMut, StoreRef,
    macros::backend::match_rt,
};
use std::{any::Any, marker::PhantomData};

#[derive(Debug, derive_more::From)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub enum BackendFunctionEnv<T> {
    #[cfg(feature = "sys")]
    /// The function environment for the `sys` runtime.
    Sys(crate::backend::sys::function::env::FunctionEnv<T>),
    #[cfg(feature = "wamr")]
    /// The function environment for the `wamr` runtime.
    Wamr(crate::backend::wamr::function::env::FunctionEnv<T>),
    #[cfg(feature = "wasmi")]
    /// The function environment for the `wasmi` runtime.
    Wasmi(crate::backend::wasmi::function::env::FunctionEnv<T>),
    #[cfg(feature = "v8")]
    /// The function environment for the `v8` runtime.
    V8(crate::backend::v8::function::env::FunctionEnv<T>),
    #[cfg(feature = "js")]
    /// The function environment for the `js` runtime.
    Js(crate::backend::js::function::env::FunctionEnv<T>),
    #[cfg(feature = "jsc")]
    /// The function environment for the `jsc` runtime.
    Jsc(crate::backend::jsc::function::env::FunctionEnv<T>),
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
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
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

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        match_rt!(on self => f {
            f.as_ref(store)
        })
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + Send + 'static + Sized,
    {
        match_rt!(on self => s {
            s.as_mut(store)
        })
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T>
    where
        T: Any + Send + 'static + Sized,
    {
        match_rt!(on self => f {
            f.into_mut(store).into()
        })
    }
}

/// A temporary handle to a [`FunctionEnv`].
#[derive(derive_more::From)]
pub enum BackendFunctionEnvMut<'a, T: 'a> {
    #[cfg(feature = "sys")]
    /// The function environment for the `sys` runtime.
    Sys(crate::backend::sys::function::env::FunctionEnvMut<'a, T>),
    #[cfg(feature = "wamr")]
    /// The function environment for the `wamr` runtime.
    Wamr(crate::backend::wamr::function::env::FunctionEnvMut<'a, T>),

    #[cfg(feature = "wasmi")]
    /// The function environment for the `wasmi` runtime.
    Wasmi(crate::backend::wasmi::function::env::FunctionEnvMut<'a, T>),
    #[cfg(feature = "v8")]
    /// The function environment for the `v8` runtime.
    V8(crate::backend::v8::function::env::FunctionEnvMut<'a, T>),

    #[cfg(feature = "js")]
    /// The function environment for the `js` runtime.
    Js(crate::backend::js::function::env::FunctionEnvMut<'a, T>),

    #[cfg(feature = "jsc")]
    /// The function environment for the `jsc` runtime.
    Jsc(crate::backend::jsc::function::env::FunctionEnvMut<'a, T>),
}

impl<T: Send + 'static> BackendFunctionEnvMut<'_, T> {
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
            Self::Sys(f) => BackendFunctionEnv::Sys(f.as_ref()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(f) => BackendFunctionEnv::Wamr(f.as_ref()).into(),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(f) => BackendFunctionEnv::Wasmi(f.as_ref()).into(),
            #[cfg(feature = "v8")]
            Self::V8(f) => BackendFunctionEnv::V8(f.as_ref()).into(),
            #[cfg(feature = "js")]
            Self::Js(f) => BackendFunctionEnv::Js(f.as_ref()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(f) => BackendFunctionEnv::Jsc(f.as_ref()).into(),
        }
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => BackendFunctionEnvMut::Sys(f.as_mut()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(f) => BackendFunctionEnvMut::Wamr(f.as_mut()).into(),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(f) => BackendFunctionEnvMut::Wasmi(f.as_mut()).into(),
            #[cfg(feature = "v8")]
            Self::V8(f) => BackendFunctionEnvMut::V8(f.as_mut()).into(),
            #[cfg(feature = "js")]
            Self::Js(f) => BackendFunctionEnvMut::Js(f.as_mut()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(f) => BackendFunctionEnvMut::Jsc(f.as_mut()).into(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        match_rt!(on self => f {
            f.data_and_store_mut()
        })
    }

    /// Creates an [`AsStoreAsync`] from this [`AsyncFunctionEnvMut`] if the current
    /// context is async.
    pub fn as_store_async(&self) -> Option<impl AsStoreAsync + 'static> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.as_store_async(),
            _ => unsupported_async_backend::<Option<crate::StoreAsync>>(),
        }
    }
}

impl<T> AsStoreRef for BackendFunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match_rt!(on self => s {
            s.as_store_ref()
        })
    }
}

impl<T> AsStoreMut for BackendFunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        match_rt!(on self => s {
            s.as_store_mut()
        })
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        match_rt!(on self => s {
            s.objects_mut()
        })
    }
}

impl<T> std::fmt::Debug for BackendFunctionEnvMut<'_, T>
where
    T: Send + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match_rt!(on self => s {
            write!(f, "{s:?}")
        })
    }
}

/// A shared handle to a [`FunctionEnv`], suitable for use
/// in async imports.
#[derive(derive_more::From)]
#[non_exhaustive]
pub enum BackendAsyncFunctionEnvMut<T> {
    #[cfg(feature = "sys")]
    /// The function environment for the `sys` runtime.
    Sys(crate::backend::sys::function::env::AsyncFunctionEnvMut<T>),
    #[cfg(not(feature = "sys"))]
    /// Placeholder for unsupported backends.
    Unsupported(PhantomData<T>),
}

/// A read-only handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[non_exhaustive]
pub enum BackendAsyncFunctionEnvHandle<T> {
    #[cfg(feature = "sys")]
    /// The function environment handle for the `sys` runtime.
    Sys(crate::backend::sys::function::env::AsyncFunctionEnvHandle<T>),
    #[cfg(not(feature = "sys"))]
    /// Placeholder for unsupported backends.
    Unsupported(PhantomData<T>),
}

/// A mutable handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[non_exhaustive]
pub enum BackendAsyncFunctionEnvHandleMut<T> {
    #[cfg(feature = "sys")]
    /// The function environment handle for the `sys` runtime.
    Sys(crate::backend::sys::function::env::AsyncFunctionEnvHandleMut<T>),
    #[cfg(not(feature = "sys"))]
    /// Placeholder for unsupported backends.
    Unsupported(PhantomData<T>),
}

impl<T: 'static> BackendAsyncFunctionEnvMut<T> {
    /// Waits for a store lock and returns a read-only handle to the
    /// function environment.
    pub async fn read(&self) -> BackendAsyncFunctionEnvHandle<T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => BackendAsyncFunctionEnvHandle::Sys(f.read().await),
            _ => unsupported_async_backend(),
        }
    }

    /// Waits for a store lock and returns a mutable handle to the
    /// function environment.
    pub async fn write(&self) -> BackendAsyncFunctionEnvHandleMut<T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => BackendAsyncFunctionEnvHandleMut::Sys(f.write().await),
            _ => unsupported_async_backend(),
        }
    }

    /// Borrows a new immutable reference
    pub fn as_ref(&self) -> BackendFunctionEnv<T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => BackendFunctionEnv::Sys(f.as_ref()),
            _ => unsupported_async_backend(),
        }
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> Self {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => Self::Sys(f.as_mut()),
            _ => unsupported_async_backend(),
        }
    }

    /// Creates an [`AsStoreAsync`] from this [`BackendAsyncFunctionEnvMut`].
    pub fn as_store_async(&self) -> impl AsStoreAsync + 'static {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.as_store_async(),
            _ => unsupported_async_backend::<crate::StoreAsync>(),
        }
    }
}

impl<T: 'static> BackendAsyncFunctionEnvHandle<T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.data(),
            _ => unsupported_async_backend(),
        }
    }

    /// Returns both the host state and the attached StoreRef
    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.data_and_store(),
            _ => unsupported_async_backend::<(&T, &StoreRef)>(),
        }
    }
}

impl<T: 'static> AsStoreRef for BackendAsyncFunctionEnvHandle<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => AsStoreRef::as_store_ref(f),
            _ => unsupported_async_backend(),
        }
    }
}

impl<T: 'static> BackendAsyncFunctionEnvHandleMut<T> {
    /// Returns a mutable reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.data_mut(),
            _ => unsupported_async_backend(),
        }
    }

    /// Returns both the host state and the attached StoreMut
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => f.data_and_store_mut(),
            _ => unsupported_async_backend::<(&mut T, &mut crate::StoreMut)>(),
        }
    }
}

impl<T: 'static> AsStoreRef for BackendAsyncFunctionEnvHandleMut<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => AsStoreRef::as_store_ref(f),
            _ => unsupported_async_backend(),
        }
    }
}

impl<T: 'static> AsStoreMut for BackendAsyncFunctionEnvHandleMut<T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => AsStoreMut::as_store_mut(f),
            _ => unsupported_async_backend(),
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => AsStoreMut::objects_mut(f),
            _ => unsupported_async_backend(),
        }
    }
}

fn unsupported_async_backend<T>() -> T {
    panic!("async functions are only supported with the `sys` backend");
}
