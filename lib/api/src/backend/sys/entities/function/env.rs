use std::{any::Any, fmt::Debug, marker::PhantomData};

use crate::{
    AsAsyncStore, AsyncStoreReadLock, AsyncStoreWriteLock, Store, StoreContext, StoreMut,
    StoreMutGuard, StoreMutWrapper,
    store::{AsStoreMut, AsStoreRef, StoreRef},
};

use wasmer_vm::{StoreHandle, StoreId, StoreObject, StoreObjects, VMFunctionEnvironment};

#[derive(Debug)]
#[repr(transparent)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub struct FunctionEnv<T> {
    pub(crate) handle: StoreHandle<VMFunctionEnvironment>,
    marker: PhantomData<T>,
}

impl<T> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        Self {
            handle: StoreHandle::new(
                store.objects_mut().as_sys_mut(),
                VMFunctionEnvironment::new(value),
            ),
            marker: PhantomData,
        }
    }

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T
    where
        T: Any + 'static + Sized,
    {
        self.handle
            .get(store.objects().as_sys())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

    #[allow(dead_code)] // This function is only used in js
    pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
        Self {
            handle,
            marker: PhantomData,
        }
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + 'static + Sized,
    {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .as_mut()
            .downcast_mut::<T>()
            .unwrap()
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T>
    where
        T: Any + 'static + Sized,
    {
        FunctionEnvMut {
            store_mut: store.reborrow_mut(),
            func_env: self,
        }
    }
}

impl<T> crate::FunctionEnv<T> {
    /// Consume self into [`crate::backend::sys::function::FunctionEnv`].
    pub fn into_sys(self) -> FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Sys(s) => s,
            _ => panic!("Not a `sys` function env!"),
        }
    }

    /// Convert a reference to self into a reference to [`crate::backend::sys::function::FunctionEnv`].
    pub fn as_sys(&self) -> &FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Sys(ref s) => s,
            _ => panic!("Not a `sys` function env!"),
        }
    }

    /// Convert a mutable reference to self into a mutable reference [`crate::backend::sys::function::FunctionEnv`].
    pub fn as_sys_mut(&mut self) -> &mut FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Sys(ref mut s) => s,
            _ => panic!("Not a `sys` function env!"),
        }
    }
}

impl<T> PartialEq for FunctionEnv<T> {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl<T> Eq for FunctionEnv<T> {}

impl<T> std::hash::Hash for FunctionEnv<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.handle.hash(state);
        self.marker.hash(state);
    }
}

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            marker: self.marker,
        }
    }
}

/// A temporary handle to a [`FunctionEnv`].
pub struct FunctionEnvMut<'a, T: 'a> {
    pub(crate) store_mut: &'a mut StoreMut,
    pub(crate) func_env: FunctionEnv<T>,
}

impl<T> Debug for FunctionEnvMut<'_, T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.func_env.as_ref(&self.store_mut).fmt(f)
    }
}

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this function environement.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.store_mut)
    }

    /// Returns a mutable- reference to the host state in this function environement.
    pub fn data_mut(&mut self) -> &mut T {
        self.func_env.as_mut(&mut self.store_mut)
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.func_env.clone()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store_mut: self.store_mut.reborrow_mut(),
            func_env: self.func_env.clone(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut StoreMut) {
        let data = self.func_env.as_mut(&mut self.store_mut) as *mut T;
        // telling the borrow check to close his eyes here
        // this is still relatively safe to do as func_env are
        // stored in a specific vec of Store, separate from the other objects
        // and not really directly accessible with the StoreMut
        let data = unsafe { &mut *data };
        (data, self.store_mut.reborrow_mut())
    }

    /// Creates an [`AsAsyncStore`] from this [`FunctionEnvMut`].
    pub fn as_async_store(&mut self) -> impl AsAsyncStore + 'static {
        Store {
            id: self.store_mut.store_handle.id,
            inner: self.store_mut.store_handle.inner.clone(),
        }
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_ref(&self) -> &crate::StoreInner {
        self.store_mut.as_ref()
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_mut(&mut self) -> &mut crate::StoreInner {
        self.store_mut.as_mut()
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        self.store_mut.reborrow_mut()
    }
}

impl<'a, T> From<FunctionEnvMut<'a, T>> for crate::FunctionEnvMut<'a, T> {
    fn from(value: FunctionEnvMut<'a, T>) -> Self {
        crate::FunctionEnvMut(crate::BackendFunctionEnvMut::Sys(value))
    }
}

impl<T> From<FunctionEnv<T>> for crate::FunctionEnv<T> {
    fn from(value: FunctionEnv<T>) -> Self {
        Self(crate::BackendFunctionEnv::Sys(value))
    }
}

/// A temporary handle to a [`FunctionEnv`], suitable for use
/// in async imports.
pub struct AsyncFunctionEnvMut<T> {
    pub(crate) store: Store,
    pub(crate) func_env: FunctionEnv<T>,
}

/// A read-only handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
pub struct AsyncFunctionEnvHandle<'a, T> {
    read_lock: AsyncStoreReadLock<'a>,
    pub(crate) func_env: FunctionEnv<T>,

    // This type needs to be !Send
    _marker: PhantomData<*const &'a ()>,
}

/// A mutable handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
/// Internally, a [`StoreMutGuard`] is used, so the store handle from this
/// type can be used to invoke [`Function::call`](crate::Function::call)
/// while outside a store's context.
pub struct AsyncFunctionEnvHandleMut<'a, T> {
    write_lock: AsyncStoreWriteLock<'a>,
    pub(crate) func_env: FunctionEnv<T>,

    // This type needs to be !Send
    _marker: PhantomData<*const ()>,
}

impl<T> Debug for AsyncFunctionEnvMut<T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.store.try_make_mut() {
            Some(store_mut) => self.func_env.as_ref(&store_mut).fmt(f),
            None => write!(f, "AsyncFunctionEnvMut {{ <STORE LOCKED> }}"),
        }
    }
}

impl<T: 'static> AsyncFunctionEnvMut<T> {
    pub(crate) fn store_id(&self) -> StoreId {
        self.store.id
    }

    /// Waits for a store lock and returns a read-only handle to the
    /// function environment.
    pub async fn read<'a>(&'a self) -> AsyncFunctionEnvHandle<'a, T> {
        let read_lock = self.store.read_lock().await;
        AsyncFunctionEnvHandle {
            read_lock,
            func_env: self.func_env.clone(),
            _marker: PhantomData,
        }
    }

    /// Waits for a store lock and returns a mutable handle to the
    /// function environment.
    pub async fn write<'a>(&'a self) -> AsyncFunctionEnvHandleMut<'a, T> {
        let write_lock = self.store.write_lock().await;
        AsyncFunctionEnvHandleMut {
            write_lock,
            func_env: self.func_env.clone(),
            _marker: PhantomData,
        }
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.func_env.clone()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> Self {
        Self {
            store: Store {
                id: self.store.id,
                inner: self.store.inner.clone(),
            },
            func_env: self.func_env.clone(),
        }
    }

    /// Creates an [`AsAsyncStore`] from this [`AsyncFunctionEnvMut`].
    pub fn as_async_store(&mut self) -> impl AsAsyncStore + 'static {
        Store {
            id: self.store.id,
            inner: self.store.inner.clone(),
        }
    }
}

impl<T: 'static> AsyncFunctionEnvHandle<'_, T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.read_lock)
    }

    /// Returns both the host state and the attached StoreRef
    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        (self.data(), &self.read_lock)
    }
}

impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandle<'_, T> {
    fn as_ref(&self) -> &crate::StoreInner {
        AsStoreRef::as_ref(&self.read_lock)
    }
}

impl<T: 'static> AsyncFunctionEnvHandleMut<'_, T> {
    /// Returns a mutable reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        self.func_env.as_mut(&mut self.write_lock)
    }

    /// Returns both the host state and the attached StoreMut
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        let data = self.data_mut() as *mut T;
        // Wisdom of the ancients:
        // telling the borrow check to close his eyes here
        // this is still relatively safe to do as func_env are
        // stored in a specific vec of Store, separate from the other objects
        // and not really directly accessible with the StoreMut
        let data = unsafe { &mut *data };
        (data, &mut self.write_lock)
    }
}

impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandleMut<'_, T> {
    fn as_ref(&self) -> &crate::StoreInner {
        AsStoreRef::as_ref(&self.write_lock)
    }
}

impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<'_, T> {
    fn as_mut(&mut self) -> &mut crate::StoreInner {
        AsStoreMut::as_mut(&mut self.write_lock)
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        AsStoreMut::reborrow_mut(&mut self.write_lock)
    }

    fn take(&mut self) -> Option<StoreMut> {
        AsStoreMut::take(&mut self.write_lock)
    }

    fn put_back(&mut self, store_mut: StoreMut) {
        AsStoreMut::put_back(&mut self.write_lock, store_mut);
    }
}
