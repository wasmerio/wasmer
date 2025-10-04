use std::marker::PhantomData;

use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::{StoreMut, StoreRef};

/// Placeholder function environment handle for the stub backend.
#[derive(Clone, Debug, Default)]
pub struct FunctionEnv<T>(PhantomData<T>);

impl<T> FunctionEnv<T> {
    pub fn new(_store: &mut impl AsStoreMut, _value: T) -> Self
    where
        T: Send + 'static,
    {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn as_ref<'a>(&self, _store: &'a impl AsStoreRef) -> &'a T {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn as_mut<'a>(&self, _store: &'a mut impl AsStoreMut) -> &'a mut T {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn into_mut(self, _store: &mut impl AsStoreMut) -> FunctionEnvMut<'static, T>
    where
        T: Send + 'static,
    {
        panic!("Function environments are unsupported by the stub backend")
    }
}

/// Placeholder mutable environment handle for the stub backend.
#[derive(Clone, Debug, Default)]
pub struct FunctionEnvMut<'a, T>(PhantomData<&'a mut T>);

impl<'a, T> FunctionEnvMut<'a, T> {
    pub fn data(&self) -> &T {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn data_mut(&mut self) -> &mut T {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn as_ref(&self) -> FunctionEnv<T>
    where
        T: Send + 'static,
    {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T>
    where
        T: Send + 'static,
    {
        panic!("Function environments are unsupported by the stub backend")
    }

    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        panic!("Function environments are unsupported by the stub backend")
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        panic!("Function environments are unsupported by the stub backend")
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        panic!("Function environments are unsupported by the stub backend")
    }

    fn objects_mut(&mut self) -> &mut crate::entities::store::StoreObjects {
        panic!("Function environments are unsupported by the stub backend")
    }
}
