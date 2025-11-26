pub(crate) mod inner;
pub(crate) use inner::*;

use crate::{AsAsyncStore, AsStoreMut, AsStoreRef, StoreMut, StoreRef, macros::backend::match_rt};
use std::{any::Any, fmt::Debug, marker::PhantomData};

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
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        Self(BackendFunctionEnv::new(store, value))
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
        self.0.as_ref(store)
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + Send + 'static + Sized,
    {
        self.0.as_mut(store)
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T>
    where
        T: Any + Send + 'static + Sized,
    {
        self.0.into_mut(store)
    }
}

/// A temporary handle to a [`FunctionEnv`].
#[derive(derive_more::From)]
pub struct FunctionEnvMut<'a, T: 'a>(pub(crate) BackendFunctionEnvMut<'a, T>);

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
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
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        self.0.as_mut()
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        self.0.data_and_store_mut()
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.0.as_store_ref()
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.0.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        self.0.objects_mut()
    }
}

impl<T> std::fmt::Debug for FunctionEnvMut<'_, T>
where
    T: Send + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A temporary handle to a [`FunctionEnv`], suitable for use
/// in async imports.
pub struct AsyncFunctionEnvMut<T>(pub(crate) BackendAsyncFunctionEnvMut<T>);

/// A read-only handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
pub struct AsyncFunctionEnvHandle<'a, T>(pub(crate) BackendAsyncFunctionEnvHandle<'a, T>);

/// A mutable handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
/// Internally, a [`StoreMutGuard`] is used, so the store handle from this
/// type can be used to invoke [`Function::call`](crate::Function::call)
/// while outside a store's context.
pub struct AsyncFunctionEnvHandleMut<'a, T>(pub(crate) BackendAsyncFunctionEnvHandleMut<'a, T>);

impl<T: 'static> AsyncFunctionEnvMut<T> {
    /// Waits for a store lock and returns a read-only handle to the
    /// function environment.
    pub async fn read<'a>(&'a self) -> AsyncFunctionEnvHandle<'a, T> {
        AsyncFunctionEnvHandle(self.0.read().await)
    }

    /// Waits for a store lock and returns a mutable handle to the
    /// function environment.
    pub async fn write<'a>(&'a self) -> AsyncFunctionEnvHandleMut<'a, T> {
        AsyncFunctionEnvHandleMut(self.0.write().await)
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        FunctionEnv(self.0.as_ref())
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> AsyncFunctionEnvMut<T> {
        AsyncFunctionEnvMut(self.0.as_mut())
    }

    /// Creates an [`AsAsyncStore`] from this [`AsyncFunctionEnvMut`].
    pub fn as_async_store(&mut self) -> impl AsAsyncStore + 'static {
        self.0.as_async_store()
    }
}

impl<T: 'static> AsyncFunctionEnvHandle<'_, T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.0.data()
    }

    /// Returns both the host state and the attached StoreRef
    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        self.0.data_and_store()
    }
}

impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandle<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.0)
    }
}

impl<T: 'static> AsyncFunctionEnvHandleMut<'_, T> {
    /// Returns a mutable reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }

    /// Returns both the host state and the attached StoreMut
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        self.0.data_and_store_mut()
    }
}

impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandleMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.0)
    }
}

impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        AsStoreMut::as_store_mut(&mut self.0)
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        AsStoreMut::objects_mut(&mut self.0)
    }
}
