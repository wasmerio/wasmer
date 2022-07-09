use std::{any::Any, marker::PhantomData};

use wasmer_vm::{StoreHandle, StoreObjects, VMFunctionEnvironment};

use crate::{AsStoreMut, AsStoreRef, Store, StoreMut, StoreRef};

use super::store::StoreInner;

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

    /// Get the context as mutable
    pub fn as_mut<'a>(&self, store: &'a mut StoreMut) -> &'a mut T
    // FunctionEnvMut<'a, T>
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get_mut(store.objects_mut())
            .as_mut()
            .downcast_mut::<T>()
            .unwrap()
        // unsafe {
        //     FunctionEnvMut {
        //         store_mut: StoreMut {
        //             inner: &mut *store_mut.as_raw(),
        //         },
        //         data,
        //     }
        // }
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
}
