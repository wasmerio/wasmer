use std::{any::Any, fmt::Debug, marker::PhantomData};

use crate::{
    AsStoreAsync, AsyncStoreReadLock, AsyncStoreWriteLock, Store, StoreAsync, StoreContext,
    StoreMut,
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

impl<T: Any + Send + 'static + Sized> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self {
        Self {
            handle: StoreHandle::new(
                store.as_store_mut().objects_mut().as_sys_mut(),
                VMFunctionEnvironment::new(value),
            ),
            marker: PhantomData,
        }
    }

    #[allow(dead_code)] // This function is only used in js
    pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
        Self {
            handle,
            marker: PhantomData,
        }
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store_mut: store.as_store_mut(),
            func_env: self,
        }
    }
}

impl<T: Any + 'static + Sized> FunctionEnv<T> {
    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T {
        self.handle
            .get(store.as_store_ref().objects().as_sys())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T {
        self.handle
            .get_mut(store.objects_mut().as_sys_mut())
            .as_mut()
            .downcast_mut::<T>()
            .unwrap()
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
    pub(crate) store_mut: StoreMut<'a>,
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
            store_mut: self.store_mut.as_store_mut(),
            func_env: self.func_env.clone(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        let data = self.func_env.as_mut(&mut self.store_mut) as *mut T;
        // telling the borrow check to close his eyes here
        // this is still relatively safe to do as func_env are
        // stored in a specific vec of Store, separate from the other objects
        // and not really directly accessible with the StoreMut
        let data = unsafe { &mut *data };
        (data, self.store_mut.as_store_mut())
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

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        self.store_mut.objects_mut()
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
    pub(crate) store: StoreAsync,
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
        match self.store.inner.try_read_rc() {
            Some(read_lock) => self.func_env.as_ref(&read_lock).fmt(f),
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
    pub fn as_mut(&mut self) -> AsyncFunctionEnvMut<T> {
        AsyncFunctionEnvMut {
            store: StoreAsync {
                id: self.store.id,
                inner: self.store.inner.clone(),
            },
            func_env: self.func_env.clone(),
        }
    }

    /// Creates an [`AsStoreAsync`] from this [`AsyncFunctionEnvMut`].
    pub fn as_store_async(&self) -> impl AsStoreAsync + 'static {
        StoreAsync {
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
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.read_lock)
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
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.write_lock)
    }
}

impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        AsStoreMut::as_store_mut(&mut self.write_lock)
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        AsStoreMut::objects_mut(&mut self.write_lock)
    }
}
