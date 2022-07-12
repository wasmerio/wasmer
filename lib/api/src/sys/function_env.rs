use std::{any::Any, marker::PhantomData};

use wasmer_vm::{StoreHandle, StoreObjects, VMFunctionEnvironment};

use crate::{AsStoreMut, AsStoreRef, StoreMut, StoreRef};

#[derive(Debug)]
#[repr(transparent)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub struct FunctionEnv<T> {
    pub(crate) handle: StoreHandle<VMFunctionEnvironment>,
    _phantom: PhantomData<T>,
}

impl<T> FunctionEnv<T> {
    /// Make a new extern reference
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        Self {
            handle: StoreHandle::new(
                store.as_store_mut().objects_mut(),
                VMFunctionEnvironment::new(value),
            ),
            _phantom: PhantomData,
        }
    }

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreMut) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get(store.as_store_ref().objects())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get_mut(store.objects_mut())
            .as_mut()
            .downcast_mut::<T>()
            .unwrap()
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<T>
    where
        T: Any + Send + 'static + Sized,
    {
        FunctionEnvMut {
            store_mut: store.as_store_mut(),
            func_env: self,
        }
    }
}

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            _phantom: self._phantom,
        }
    }
}

/// A temporary handle to a [`Context`].
pub struct FunctionEnvMut<'a, T: 'a> {
    pub(crate) store_mut: StoreMut<'a>,
    pub(crate) func_env: FunctionEnv<T>,
}

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.store_mut)
    }

    /// Returns a mutable- reference to the host state in this context.
    pub fn data_mut<'a>(&'a mut self) -> &'a mut T {
        self.func_env.as_mut(&mut self.store_mut)
    }

    /// Borrows a new mutable reference
    pub fn as_mut<'a>(&'a mut self) -> FunctionEnvMut<'a, T> {
        FunctionEnvMut {
            store_mut: self.store_mut.as_store_mut(),
            func_env: self.func_env.clone(),
        }
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef {
            inner: self.store_mut.inner,
        }
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut {
            inner: self.store_mut.inner,
        }
    }
    #[inline]
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.store_mut.inner.objects
    }
}
