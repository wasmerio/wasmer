pub(crate) mod inner;
pub(crate) use inner::*;

use crate::{macros::backend::match_rt, AsStoreMut, AsStoreRef, StoreMut, StoreRef};
use std::{any::Any, fmt::Debug, marker::PhantomData};

use wasmer_types::{BoxStoreObject, Upcast};

#[derive(Debug, derive_more::From)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub struct FunctionEnv<T>(pub(crate) BackendFunctionEnv<T>);

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut<Object: Upcast<T>>, value: T) -> Self {
        Self(BackendFunctionEnv::new(store, value))
    }

    //#[allow(dead_code)] // This function is only used in js
    //pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
    //    todo!()
    //}

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef<Object: Upcast<T>>) -> &'a T {
        self.0.as_ref(store)
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut<Object: Upcast<T>>) -> &'a mut T {
        self.0.as_mut(store)
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut<S>(self, store: &mut S) -> FunctionEnvMut<'_, T, S::Object>
    where
        S: AsStoreMut<Object: Upcast<T>>,
    {
        self.0.into_mut(store)
    }
}

/// A temporary handle to a [`FunctionEnv`].
#[derive(derive_more::From)]
pub struct FunctionEnvMut<'a, T, Object = BoxStoreObject>(pub(crate) BackendFunctionEnvMut<'a, T, Object>);

impl<T, Object: Upcast<T>> FunctionEnvMut<'_, T, Object> {
    /// Returns a reference to the host state in this function environement.
    pub fn data(&self) -> &T {
        self.0.data()
    }

    /// Returns a mutable- reference to the host state in this function environement.
    pub fn data_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.0.as_ref()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T, Object> {
        self.0.as_mut()
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_, Object>) {
        self.0.data_and_store_mut()
    }
}

impl<T, Object> AsStoreRef for FunctionEnvMut<'_, T, Object> {
    type Object = Object;

    fn as_store_ref(&self) -> StoreRef<'_, Object> {
        self.0.as_store_ref()
    }
}

impl<T, Object> AsStoreMut for FunctionEnvMut<'_, T, Object> {
    fn as_store_mut(&mut self) -> StoreMut<'_, Object> {
        self.0.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects<Object> {
        self.0.objects_mut()
    }
}

impl<T: std::fmt::Debug, Object: Upcast<T>> std::fmt::Debug for FunctionEnvMut<'_, T, Object> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
