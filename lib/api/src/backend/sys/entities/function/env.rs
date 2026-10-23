use std::{any::Any, fmt::Debug, marker::PhantomData};

#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync, StoreAsyncReadLock, StoreAsyncWriteLock};
use crate::{
    Store, StoreContext, StoreInner, StoreMut, StorePtrWrapper,
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
            store: EnvStoreMut::Borrowed(store.as_store_mut()),
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

/// How a [`FunctionEnvMut`] reaches its store.
pub(crate) enum EnvStoreMut<'a> {
    /// A borrow the caller handed in, as [`FunctionEnv::into_mut`] does.
    Borrowed(StoreMut<'a>),

    /// The store executing on this thread, re-derived on every access.
    ///
    /// An import trampoline uses this rather than holding the borrow it
    /// acquired. The host function it hands the environment to can re-enter the
    /// guest, and under an async store that nested call can suspend — at which
    /// point the runtime releases the store so another task can take it, and a
    /// borrow held across that is invalidated by whoever takes it next.
    /// Deriving per access keeps nothing alive over the boundary, which is the
    /// same reason `async_runtime`'s `AsyncCallStoreMut` holds no borrow.
    Context {
        id: StoreId,
        marker: PhantomData<&'a mut StoreInner>,
    },
}

impl EnvStoreMut<'_> {
    /// Reborrows for a shorter lifetime, so a nested handle reaches the store
    /// the same way this one does.
    fn reborrow(&mut self) -> EnvStoreMut<'_> {
        match self {
            Self::Borrowed(store) => EnvStoreMut::Borrowed(store.as_store_mut()),
            Self::Context { id, .. } => EnvStoreMut::Context {
                id: *id,
                marker: PhantomData,
            },
        }
    }
}

impl AsStoreRef for EnvStoreMut<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match self {
            Self::Borrowed(store) => store.as_store_ref(),
            // Safety: building this variant requires a context for `id` to stay
            // installed for `'a`, and the reference does not outlive `&self`.
            Self::Context { id, .. } => StoreRef {
                inner: unsafe { &*StoreContext::get_current_transient(*id) },
            },
        }
    }
}

impl AsStoreMut for EnvStoreMut<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        match self {
            Self::Borrowed(store) => store.as_store_mut(),
            // Safety: as in `as_store_ref`.
            Self::Context { id, .. } => StoreMut {
                inner: unsafe { &mut *StoreContext::get_current_transient(*id) },
            },
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        match self {
            Self::Borrowed(store) => store.objects_mut(),
            // Safety: as in `as_store_ref`.
            Self::Context { id, .. } => unsafe {
                &mut (*StoreContext::get_current_transient(*id)).objects
            },
        }
    }
}

/// A temporary handle to a [`FunctionEnv`].
pub struct FunctionEnvMut<'a, T: 'a> {
    pub(crate) store: EnvStoreMut<'a>,
    pub(crate) func_env: FunctionEnv<T>,
}

impl<'a, T> FunctionEnvMut<'a, T> {
    /// Builds a handle that reaches the store through this thread's store
    /// context instead of holding a borrow of it. Import trampolines use this;
    /// see [`EnvStoreMut::Context`].
    ///
    /// # Safety
    /// A store context for `id` must stay installed on this thread for all of
    /// `'a`.
    pub(crate) unsafe fn from_context(id: StoreId, func_env: FunctionEnv<T>) -> Self {
        Self {
            store: EnvStoreMut::Context {
                id,
                marker: PhantomData,
            },
            func_env,
        }
    }
}

impl<T> Debug for FunctionEnvMut<'_, T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.func_env.as_ref(&self.store).fmt(f)
    }
}

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.store)
    }

    /// Returns a mutable- reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        self.func_env.as_mut(&mut self.store)
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.func_env.clone()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store: self.store.reborrow(),
            func_env: self.func_env.clone(),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        let data = self.func_env.as_mut(&mut self.store) as *mut T;
        // telling the borrow check to close his eyes here
        // this is still relatively safe to do as func_env are
        // stored in a specific vec of Store, separate from the other objects
        // and not really directly accessible with the StoreMut
        let data = unsafe { &mut *data };
        (data, self.store.as_store_mut())
    }

    /// Returns a [`StoreAsync`] if the current
    /// context is asynchronous. The store will be locked since
    /// it's already active in the current context, but can be used
    /// to spawn new coroutines via
    /// [`Function::call_async`](crate::Function::call_async).
    #[cfg(feature = "experimental-async")]
    pub fn as_store_async(&self) -> Option<impl AsStoreAsync + 'static> {
        self.store.as_store_async()
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.store.as_store_ref()
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.store.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        self.store.objects_mut()
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

/// A shared handle to a [`FunctionEnv`], suitable for use
/// in async imports.
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvMut<T> {
    pub(crate) store: AsyncFunctionEnvMutStore,
    pub(crate) func_env: FunctionEnv<T>,
}

// We need to let async functions that *don't suspend* run
// in a sync context. To that end, `AsyncFunctionEnvMut`
// must be able to be constructed without an actual
// StoreAsync instance, hence this enum.
#[cfg(feature = "experimental-async")]
pub(crate) enum AsyncFunctionEnvMutStore {
    Async(StoreAsync),
    Sync(StorePtrWrapper),
}

/// A read-only handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandle<T> {
    read_lock: StoreAsyncReadLock,
    pub(crate) func_env: FunctionEnv<T>,
}

/// A mutable handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandleMut<T> {
    write_lock: StoreAsyncWriteLock,
    pub(crate) func_env: FunctionEnv<T>,
}

#[cfg(feature = "experimental-async")]
impl<T> Debug for AsyncFunctionEnvMut<T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.store {
            AsyncFunctionEnvMutStore::Sync(ptr) => self.func_env.as_ref(&ptr.as_ref()).fmt(f),
            AsyncFunctionEnvMutStore::Async(store) => match store.inner.try_read() {
                Some(read_lock) => self.func_env.as_ref(&read_lock).fmt(f),
                None => write!(f, "AsyncFunctionEnvMut {{ <STORE LOCKED> }}"),
            },
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvMut<T> {
    pub(crate) fn store_id(&self) -> StoreId {
        match &self.store {
            AsyncFunctionEnvMutStore::Sync(ptr) => ptr.as_ref().objects().id(),
            AsyncFunctionEnvMutStore::Async(store) => store.id,
        }
    }

    /// Waits for a store lock and returns a read-only handle to the
    /// function environment.
    pub async fn read(&self) -> AsyncFunctionEnvHandle<T> {
        let read_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.read_lock().await,

            // We can never acquire a store lock in a sync context
            AsyncFunctionEnvMutStore::Sync(_) => futures::future::pending().await,
        };

        AsyncFunctionEnvHandle {
            read_lock,
            func_env: self.func_env.clone(),
        }
    }

    /// Waits for a store lock and returns a mutable handle to the
    /// function environment.
    pub async fn write(&self) -> AsyncFunctionEnvHandleMut<T> {
        let write_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => store.write_lock().await,

            // We can never acquire a store lock in a sync context
            AsyncFunctionEnvMutStore::Sync(_) => futures::future::pending().await,
        };

        AsyncFunctionEnvHandleMut {
            write_lock,
            func_env: self.func_env.clone(),
        }
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.func_env.clone()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> Self {
        self.clone()
    }

    /// Creates an [`AsStoreAsync`] from this [`AsyncFunctionEnvMut`].
    pub fn as_store_async(&self) -> impl AsStoreAsync + 'static {
        match &self.store {
            AsyncFunctionEnvMutStore::Sync(_) => {
                panic!("Cannot build a StoreAsync within a sync context")
            }
            AsyncFunctionEnvMutStore::Async(store) => StoreAsync {
                id: store.id,
                inner: store.inner.clone(),
            },
        }
    }
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
impl Clone for AsyncFunctionEnvMutStore {
    fn clone(&self) -> Self {
        match self {
            Self::Async(store) => Self::Async(StoreAsync {
                id: store.id,
                inner: store.inner.clone(),
            }),
            Self::Sync(ptr) => Self::Sync(ptr.clone()),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandle<T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.func_env.as_ref(&self.read_lock)
    }

    /// Returns both the host state and the attached StoreRef
    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        (self.data(), &self.read_lock)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandle<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.read_lock)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandleMut<T> {
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

    /// Borrows a new [`FunctionEnvMut`] from this [`AsyncFunctionEnvHandleMut`].
    pub fn as_function_env_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store: EnvStoreMut::Borrowed(self.write_lock.as_store_mut()),
            func_env: self.func_env.clone(),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandleMut<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.write_lock)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        AsStoreMut::as_store_mut(&mut self.write_lock)
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        AsStoreMut::objects_mut(&mut self.write_lock)
    }
}
