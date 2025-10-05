use std::marker::PhantomData;

use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::{StoreMut, StoreRef};

/// Placeholder function environment handle for the stub backend.
#[derive(Debug)]
pub struct FunctionEnv<T>(PhantomData<T>);

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

impl<T> FunctionEnv<T> {
    pub fn new(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: Send + 'static,
    {
        panic_stub("cannot create function environments")
    }

    pub fn as_ref<'a>(&self, _store: &'a impl AsStoreRef) -> &'a T {
        panic_stub("cannot access function environment data")
    }

    pub fn as_mut<'a>(&self, _store: &'a mut impl AsStoreMut) -> &'a mut T {
        panic_stub("cannot access function environment data mutably")
    }

    pub fn into_mut(self, _store: &mut impl AsStoreMut) -> FunctionEnvMut<'static, T>
    where
        T: Send + 'static,
    {
        panic_stub("cannot convert function environments")
    }
}

/// Placeholder mutable environment handle for the stub backend.
#[derive(Debug)]
pub struct FunctionEnvMut<'a, T>(PhantomData<&'a mut T>);

impl<'a, T> Clone for FunctionEnvMut<'a, T> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

impl<'a, T> FunctionEnvMut<'a, T> {
    pub fn data(&self) -> &T {
        panic_stub("cannot access function environment data")
    }

    pub fn data_mut(&mut self) -> &mut T {
        panic_stub("cannot access function environment data mutably")
    }

    pub fn as_ref(&self) -> FunctionEnv<T>
    where
        T: Send + 'static,
    {
        panic_stub("cannot convert function environments")
    }

    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T>
    where
        T: Send + 'static,
    {
        panic_stub("cannot convert function environments")
    }

    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        panic_stub("cannot access function environment data and store")
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        panic_stub("cannot expose store references from function environments")
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        panic_stub("cannot expose mutable store references from function environments")
    }

    fn objects_mut(&mut self) -> &mut crate::entities::store::StoreObjects {
        panic_stub("cannot expose store objects from function environments")
    }
}
