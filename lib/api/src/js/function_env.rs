use std::{any::Any, marker::PhantomData};
use std::sync::Arc;

use crate::js::{StoreHandle, StoreObjects}; 
use crate::js::{AsStoreMut, AsStoreRef, StoreMut, StoreRef};

use super::store::PackagedStore;

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

    pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
        Self {
            handle,
            _phantom: PhantomData,
        }
    }

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get(store.as_store_ref().objects())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

    /// Get the data as mutable reference
    /// (this will only return a mutable reference as long as the environment
    ///  has not been cloned - environments are cloned during multithreading)
    pub fn as_mut<'a>(&mut self, store: &'a mut impl AsStoreMut) -> Option<&'a mut T>
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get_mut(store.objects_mut())
            .as_mut()
            .map(|a| a.downcast_mut::<T>())
            .flatten()
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
    /// (this will only return a mutable reference as long as the environment
    ///  has not been cloned - environments are cloned during multithreading)
    pub fn data_mut<'a>(&'a mut self) -> Option<&'a mut T> {
        self.func_env.as_mut(&mut self.store_mut)
    }

    /// Borrows a new mutable reference
    pub fn as_mut<'a>(&'a mut self) -> FunctionEnvMut<'a, T> {
        FunctionEnvMut {
            store_mut: self.store_mut.as_store_mut(),
            func_env: self.func_env.clone(),
        }
    }

    /// Packages up an empty store that can be passed to another thread
    pub fn package_store(&self) -> PackagedStore {
        self.store_mut.package()
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

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment {
    contents: Arc<dyn Any + Send + 'static>,
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Arc::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_mut(&mut self) -> Option<&mut (dyn Any + Send + 'static)> {
        Arc::get_mut(&mut self.contents)
    }
}
