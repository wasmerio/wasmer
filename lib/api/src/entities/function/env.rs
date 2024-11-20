use crate::{AsStoreMut, AsStoreRef, StoreMut, StoreRef};
use std::{any::Any, fmt::Debug, marker::PhantomData};

#[derive(Debug, derive_more::From)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub enum FunctionEnv<T> {
    #[cfg(feature = "sys")]
    /// The function environment for the `sys` runtime.
    Sys(crate::rt::sys::function::env::FunctionEnv<T>),
    #[cfg(feature = "wamr")]
    /// The function environment for the `wamr` runtime.
    Wamr(crate::rt::wamr::function::env::FunctionEnv<T>),
    #[cfg(feature = "v8")]
    /// The function environment for the `v8` runtime.
    V8(crate::rt::v8::function::env::FunctionEnv<T>),
    #[cfg(feature = "js")]
    /// The function environment for the `js` runtime.
    Js(crate::rt::js::function::env::FunctionEnv<T>),
}

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => Self::Sys(s.clone()),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => Self::Wamr(s.clone()),
            #[cfg(feature = "v8")]
            Self::V8(s) => Self::V8(s.clone()),
            #[cfg(feature = "js")]
            Self::Js(s) => Self::Js(s.clone()),
        }
    }
}

impl<T> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        match store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(_) => Self::Sys(
                crate::rt::sys::function::env::FunctionEnv::new(store, value),
            ),

            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => Self::Wamr(
                crate::rt::wamr::function::env::FunctionEnv::new(store, value),
            ),

            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => {
                Self::V8(crate::rt::v8::function::env::FunctionEnv::new(store, value))
            }

            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => {
                Self::Js(crate::rt::js::function::env::FunctionEnv::new(store, value))
            }
        }
    }

    //#[allow(dead_code)] // This function is only used in js
    //pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
    //    todo!()
    //}

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.as_ref(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.as_ref(store),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.as_ref(store),
            #[cfg(feature = "js")]
            Self::Js(s) => s.as_ref(store),
        }
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + Send + 'static + Sized,
    {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.as_mut(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.as_mut(store),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.as_mut(store),
            #[cfg(feature = "js")]
            Self::Js(s) => s.as_mut(store),
        }
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<T>
    where
        T: Any + Send + 'static + Sized,
    {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => FunctionEnvMut::Sys(s.into_mut(store)),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => FunctionEnvMut::Wamr(s.into_mut(store)),
            #[cfg(feature = "v8")]
            Self::V8(s) => FunctionEnvMut::V8(s.into_mut(store)),
            #[cfg(feature = "js")]
            Self::Js(s) => FunctionEnvMut::Js(s.into_mut(store)),
        }
    }
}

/// A temporary handle to a [`FunctionEnv`].
pub enum FunctionEnvMut<'a, T: 'a> {
    #[cfg(feature = "sys")]
    /// The function environment for the `sys` runtime.
    Sys(crate::rt::sys::function::env::FunctionEnvMut<'a, T>),
    #[cfg(feature = "wamr")]
    /// The function environment for the `wamr` runtime.
    Wamr(crate::rt::wamr::function::env::FunctionEnvMut<'a, T>),
    #[cfg(feature = "v8")]
    /// The function environment for the `v8` runtime.
    V8(crate::rt::v8::function::env::FunctionEnvMut<'a, T>),

    #[cfg(feature = "js")]
    /// The function environment for the `js` runtime.
    Js(crate::rt::js::function::env::FunctionEnvMut<'a, T>),
}

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this function environement.
    pub fn data(&self) -> &T {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref f) => f.data(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref f) => f.data(),
            #[cfg(feature = "v8")]
            Self::V8(ref f) => f.data(),
            #[cfg(feature = "js")]
            Self::Js(ref f) => f.data(),
        }
    }

    /// Returns a mutable- reference to the host state in this function environement.
    pub fn data_mut(&mut self) -> &mut T {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut f) => f.data_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut f) => f.data_mut(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut f) => f.data_mut(),
            #[cfg(feature = "js")]
            Self::Js(ref mut f) => f.data_mut(),
        }
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref f) => FunctionEnv::Sys(f.as_ref()),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref f) => FunctionEnv::Wamr(f.as_ref()),
            #[cfg(feature = "v8")]
            Self::V8(ref f) => FunctionEnv::V8(f.as_ref()),
            #[cfg(feature = "js")]
            Self::Js(ref f) => FunctionEnv::Js(f.as_ref()),
        }
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut f) => FunctionEnvMut::Sys(f.as_mut()),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut f) => FunctionEnvMut::Wamr(f.as_mut()),
            #[cfg(feature = "v8")]
            Self::V8(ref mut f) => FunctionEnvMut::V8(f.as_mut()),
            #[cfg(feature = "js")]
            Self::Js(ref mut f) => FunctionEnvMut::Js(f.as_mut()),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut) {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut f) => f.data_and_store_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut f) => f.data_and_store_mut(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut f) => f.data_and_store_mut(),
            #[cfg(feature = "js")]
            Self::Js(ref mut f) => f.data_and_store_mut(),
        }
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref s) => s.as_store_ref(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref s) => s.as_store_ref(),
            #[cfg(feature = "v8")]
            Self::V8(ref s) => s.as_store_ref(),
            #[cfg(feature = "js")]
            Self::Js(ref s) => s.as_store_ref(),
        }
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut s) => s.as_store_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut s) => s.as_store_mut(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut s) => s.as_store_mut(),
            #[cfg(feature = "js")]
            Self::Js(ref mut s) => s.as_store_mut(),
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut s) => s.objects_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut s) => s.objects_mut(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut s) => s.objects_mut(),
            #[cfg(feature = "js")]
            Self::Js(ref mut s) => s.objects_mut(),
        }
    }
}

impl<'a, T> Debug for FunctionEnvMut<'a, T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => write!(f, "{s:?}"),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => write!(f, "{s:?}"),
            #[cfg(feature = "v8")]
            Self::V8(s) => write!(f, "{s:?}"),
            #[cfg(feature = "js")]
            Self::Js(s) => write!(f, "{s:?}"),
        }
    }
}
