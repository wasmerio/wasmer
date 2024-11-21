use crate::{
    macros::rt::match_rt, AsStoreMut, AsStoreRef, FunctionEnv, FunctionEnvMut, StoreMut, StoreRef,
};
use std::{any::Any, marker::PhantomData};

#[derive(Debug, derive_more::From)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub enum RuntimeFunctionEnv<T> {
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
    #[cfg(feature = "jsc")]
    /// The function environment for the `jsc` runtime.
    Jsc(crate::rt::jsc::function::env::FunctionEnv<T>),
}

impl<T> Clone for RuntimeFunctionEnv<T> {
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
            #[cfg(feature = "jsc")]
            Self::Jsc(s) => Self::Jsc(s.clone()),
        }
    }
}

impl<T> RuntimeFunctionEnv<T> {
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
            #[cfg(feature = "jsc")]
            crate::RuntimeStore::Jsc(_) => Self::Jsc(
                crate::rt::jsc::function::env::FunctionEnv::new(store, value),
            ),
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
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<T>
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
pub enum RuntimeFunctionEnvMut<'a, T: 'a> {
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

    #[cfg(feature = "jsc")]
    /// The function environment for the `jsc` runtime.
    Jsc(crate::rt::jsc::function::env::FunctionEnvMut<'a, T>),
}

impl<T: Send + 'static> RuntimeFunctionEnvMut<'_, T> {
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
            Self::Sys(ref f) => RuntimeFunctionEnv::Sys(f.as_ref()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref f) => RuntimeFunctionEnv::Wamr(f.as_ref()).into(),
            #[cfg(feature = "v8")]
            Self::V8(ref f) => RuntimeFunctionEnv::V8(f.as_ref()).into(),
            #[cfg(feature = "js")]
            Self::Js(ref f) => RuntimeFunctionEnv::Js(f.as_ref()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(ref f) => RuntimeFunctionEnv::Jsc(f.as_ref()).into(),
        }
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(ref mut f) => RuntimeFunctionEnvMut::Sys(f.as_mut()).into(),
            #[cfg(feature = "wamr")]
            Self::Wamr(ref mut f) => RuntimeFunctionEnvMut::Wamr(f.as_mut()).into(),
            #[cfg(feature = "v8")]
            Self::V8(ref mut f) => RuntimeFunctionEnvMut::V8(f.as_mut()).into(),
            #[cfg(feature = "js")]
            Self::Js(ref mut f) => RuntimeFunctionEnvMut::Js(f.as_mut()).into(),
            #[cfg(feature = "jsc")]
            Self::Jsc(ref mut f) => RuntimeFunctionEnvMut::Jsc(f.as_mut()).into(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut) {
        match_rt!(on self => f {
            f.data_and_store_mut()
        })
    }
}

impl<T> AsStoreRef for RuntimeFunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match_rt!(on &self => f {
            f.as_store_ref()
        })
    }
}

impl<T> AsStoreMut for RuntimeFunctionEnvMut<'_, T> {
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

impl<'a, T> std::fmt::Debug for RuntimeFunctionEnvMut<'a, T>
where
    T: Send + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match_rt!(on self => s {
            write!(f, "{s:?}")
        })
    }
}
