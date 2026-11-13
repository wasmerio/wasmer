use std::{any::Any, fmt::Debug, marker::PhantomData};

use crate::{
    StoreMut,
    js::{store::StoreHandle, vm::VMFunctionEnvironment},
    store::{AsStoreMut, AsStoreRef, StoreRef},
};
#[cfg(feature = "experimental-async")]
use crate::{
    AsStoreAsync, StoreAsync, StoreAsyncReadLock, StoreAsyncWriteLock, StoreInner,
};
#[cfg(feature = "experimental-async")]
use wasmer_types::StoreId;

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
                store.as_store_mut().objects_mut().as_js_mut(),
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
            .get(store.as_store_ref().objects().as_js())
            .as_ref()
            .downcast_ref::<T>()
            .unwrap()
    }

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
            .get_mut(store.objects_mut().as_js_mut())
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
            store_mut: store.as_store_mut(),
            func_env: self,
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
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.store_mut)
    }

    /// Returns a mutable- reference to the host state in this function environment.
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

    #[cfg(feature = "experimental-async")]
    pub fn as_store_async(&self) -> Option<impl AsStoreAsync + 'static> {
        self.store_mut.as_store_async()
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

impl<T> crate::FunctionEnv<T> {
    /// Consume [`self`] into [`crate::backend::js::function::env::FunctionEnv`].
    pub fn into_js(self) -> FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Js(s) => s,
            _ => panic!("Not a `js` function env!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::js::function::env::FunctionEnv`].
    pub fn as_js(&self) -> &FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Js(ref s) => s,
            _ => panic!("Not a `js` function env!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::function::env::FunctionEnv`].
    pub fn as_js_mut(&mut self) -> &mut FunctionEnv<T> {
        match self.0 {
            crate::BackendFunctionEnv::Js(ref mut s) => s,
            _ => panic!("Not a `js` function env!"),
        }
    }
}

impl<'a, T> From<FunctionEnvMut<'a, T>> for crate::FunctionEnvMut<'a, T> {
    fn from(value: FunctionEnvMut<'a, T>) -> Self {
        crate::FunctionEnvMut(crate::BackendFunctionEnvMut::Js(value))
    }
}

impl<T> From<FunctionEnv<T>> for crate::FunctionEnv<T> {
    fn from(value: FunctionEnv<T>) -> Self {
        Self(crate::BackendFunctionEnv::Js(value))
    }
}

#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvMut<T> {
    pub(crate) store: AsyncFunctionEnvMutStore,
    pub(crate) func_env: FunctionEnv<T>,
}

/// How an async host function reaches its store. Its guest was entered by
/// `Function::call_async`, or, for an eager host function only, by a
/// synchronous call: there is no [`StoreAsync`] to hand out then.
#[cfg(feature = "experimental-async")]
pub(crate) enum AsyncFunctionEnvMutStore {
    Async(StoreAsync),
    /// The store the synchronous call lent to the guest, which is only used
    /// while the host function is polled inside the import call.
    Sync(StoreId, *mut StoreInner),
}

#[cfg(feature = "experimental-async")]
impl Clone for AsyncFunctionEnvMutStore {
    fn clone(&self) -> Self {
        match self {
            Self::Async(store) => Self::Async(store.store()),
            Self::Sync(id, store) => Self::Sync(*id, *store),
        }
    }
}

#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandle<T> {
    read_lock: StoreAsyncReadLock,
    pub(crate) func_env: FunctionEnv<T>,
}

#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandleMut<T> {
    write_lock: StoreAsyncWriteLock,
    pub(crate) func_env: FunctionEnv<T>,
}

#[cfg(feature = "experimental-async")]
impl<T> Clone for AsyncFunctionEnvMut<T> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            func_env: self.func_env.clone(),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T> Debug for AsyncFunctionEnvMut<T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let read_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.inner.try_read(),
            AsyncFunctionEnvMutStore::Sync(..) => None,
        };
        match read_lock {
            Some(read_lock) => self.func_env.as_ref(&read_lock).fmt(f),
            None => write!(f, "AsyncFunctionEnvMut {{ <STORE LOCKED> }}"),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvMut<T> {
    pub(crate) fn store_id(&self) -> StoreId {
        match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.id,
            AsyncFunctionEnvMutStore::Sync(id, _) => *id,
        }
    }

    pub async fn read(&self) -> AsyncFunctionEnvHandle<T> {
        let read_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.read_lock().await,
            // There is no lock to wait for beneath a synchronous call, and
            // the guest cannot wait there either.
            AsyncFunctionEnvMutStore::Sync(..) => std::future::pending().await,
        };
        AsyncFunctionEnvHandle {
            read_lock,
            func_env: self.func_env.clone(),
        }
    }

    pub async fn write(&self) -> AsyncFunctionEnvHandleMut<T> {
        let write_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.write_lock().await,
            AsyncFunctionEnvMutStore::Sync(..) => std::future::pending().await,
        };
        AsyncFunctionEnvHandleMut {
            write_lock,
            func_env: self.func_env.clone(),
        }
    }

    /// Runs `f` with the store, as soon as it can be had: what the typed
    /// host functions convert their results with.
    pub(crate) async fn with_store_mut<R>(&self, f: impl FnOnce(&mut StoreMut<'_>) -> R) -> R {
        match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => {
                f(&mut store.write_lock().await.as_store_mut())
            }
            // SAFETY: a host function with this kind of store is polled once,
            // inside the import call, and dropped if it is not finished then.
            // The synchronous call beneath lent the store to the guest for
            // at least that long.
            AsyncFunctionEnvMutStore::Sync(_, store) => f(&mut unsafe { StoreMut::from_raw(*store) }),
        }
    }

    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.func_env.clone()
    }

    pub fn as_mut(&mut self) -> Self {
        self.clone()
    }

    pub fn as_store_async(&self) -> impl AsStoreAsync + 'static {
        match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.store(),
            AsyncFunctionEnvMutStore::Sync(..) => {
                panic!("Cannot build a StoreAsync within a sync context")
            }
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandle<T> {
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.read_lock)
    }

    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        (self.data(), &self.read_lock)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandle<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.read_lock.as_store_ref()
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandleMut<T> {
    pub fn data_mut(&mut self) -> &mut T {
        self.func_env.as_mut(&mut self.write_lock)
    }

    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        let data = self.data_mut() as *mut T;
        let data = unsafe { &mut *data };
        (data, &mut self.write_lock)
    }

    pub fn as_function_env_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store_mut: self.write_lock.as_store_mut(),
            func_env: self.func_env.clone(),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandleMut<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.write_lock.as_store_ref()
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.write_lock.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        self.write_lock.objects_mut()
    }
}
