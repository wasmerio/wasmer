use std::{any::Any, marker::PhantomData};

use wasmer_vm::{StoreHandle, VMFunctionContext};

use crate::Store;

#[derive(Debug)]
#[repr(transparent)]
/// An opaque reference to a function context.
pub struct Context<T> {
    handle: StoreHandle<VMFunctionContext>,
    _phantom: PhantomData<T>,
}

impl<T> Context<T> {
    /// Make a new extern reference
    pub fn new(store: &mut Store, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        Self {
            handle: StoreHandle::new(store.objects_mut(), VMFunctionContext::new(value)),
            _phantom: PhantomData,
        }
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a>(&self, store: &'a Store) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        self.handle
            .get(store.objects())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

    /// Try to downcast to the given value.
    pub fn downcast_mut<'a>(&self, store: &'a mut Store) -> &'a mut T
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

impl<T> Clone for Context<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            _phantom: self._phantom,
        }
    }
}
