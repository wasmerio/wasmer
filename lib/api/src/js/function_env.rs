use std::{any::Any, marker::PhantomData};

use crate::js::{StoreHandle, StoreObjects};

use crate::js::{AsStoreMut, AsStoreRef, StoreMut, StoreRef};

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

    /// Get the context as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    // FunctionEnvMut<'a, T>
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get_mut(store.objects_mut())
            .as_mut()
            .downcast_mut::<T>()
            .unwrap()
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
    pub(crate) data: &'a mut T,
}

impl<T> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this context.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Returns a mutable- reference to the host state in this context.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    // pub(crate) unsafe fn from_raw(raw: *mut StoreInner, data: *mut T) -> Self {
    //     Self { inner: &mut *raw, data: &mut *data }
    // }
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
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.store_mut.inner.objects
    }
}

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment {
    contents: Box<dyn Any + Send + 'static>,
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Box::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        &mut *self.contents
    }
}
