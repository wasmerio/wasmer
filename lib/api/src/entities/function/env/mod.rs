pub(crate) mod inner;
pub(crate) use inner::*;

#[cfg(feature = "experimental-async")]
use crate::AsStoreAsync;
use crate::{AsStoreMut, AsStoreRef, StoreMut, StoreRef, macros::backend::match_rt};
use std::{any::Any, fmt::Debug, marker::PhantomData};

#[derive(Debug, derive_more::From)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub struct FunctionEnv<T>(pub(crate) BackendFunctionEnv<T>);

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + 'static + Sized,
    {
        Self(BackendFunctionEnv::new(store, value))
    }

    //#[allow(dead_code)] // This function is only used in js
    //pub(crate) fn from_handle(handle: StoreHandle<VMFunctionEnvironment>) -> Self {
    //    todo!()
    //}

    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T
    where
        T: Any + Send + 'static + Sized,
    {
        self.0.as_ref(store)
    }

    /// Get the data as mutable
    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T
    where
        T: Any + Send + 'static + Sized,
    {
        self.0.as_mut(store)
    }

    /// Convert it into a `FunctionEnvMut`
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T>
    where
        T: Any + Send + 'static + Sized,
    {
        self.0.into_mut(store)
    }
}

/// A temporary handle to a [`FunctionEnv`].
#[derive(derive_more::From)]
pub struct FunctionEnvMut<'a, T: 'a>(pub(crate) BackendFunctionEnvMut<'a, T>);

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.0.data()
    }

    /// Returns a mutable- reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        self.0.as_ref()
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> FunctionEnvMut<'_, T> {
        self.0.as_mut()
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        self.0.data_and_store_mut()
    }

    /// Creates an [`AsStoreAsync`] from this [`FunctionEnvMut`] if the current
    /// context is async.
    #[cfg(feature = "experimental-async")]
    pub fn as_store_async(&self) -> Option<impl AsStoreAsync + 'static> {
        self.0.as_store_async()
    }

    /// A handle to this environment that outlives the call currently running,
    /// for a callback registered with a foreign runtime.
    ///
    /// `None` unless this host function was reached through
    /// [`Function::call_async`](crate::Function::call_async): there is no async
    /// store to refer to otherwise. See [`FunctionEnvHandle`].
    #[cfg(feature = "experimental-async")]
    pub fn handle(&self) -> Option<FunctionEnvHandle<T>> {
        let id = self.as_store_ref().objects().id();
        Some(FunctionEnvHandle {
            store: crate::StoreAsync::from_context(id)?.downgrade(),
            func_env: self.as_ref(),
        })
    }
}

/// A stashable handle to a [`FunctionEnv`] and the store it lives in.
///
/// A host function that registers a callback with a foreign runtime — an N-API
/// module handing a function to a JavaScript engine, say — has to reach its
/// environment again when that callback fires, which can be long after the call
/// that registered it returned, and from a stack with no Rust frame of ours on
/// it. This is that handle; mint one with [`FunctionEnvMut::handle`].
///
/// It holds the store *weakly*, so it never keeps a store alive. A strong handle
/// would deadlock ownership whenever the registration is kept in the store's own
/// data, which is where such registrations naturally live.
///
/// Reach the environment with [`Self::try_write`], which takes the store's write
/// lock. `None` means the environment is not reachable *right now*, which is an
/// ordinary outcome rather than an error: a guest may be running and holding the
/// store, or it may be gone for good ([`Self::is_alive`] separates those). A
/// caller that cannot proceed should queue the work and retry — forcing its way
/// in is what makes two live `StoreMut`s for one store, which is undefined
/// behaviour.
///
/// The moment this does succeed is while the guest is **suspended**: a guest that
/// suspends releases the store, so a callback arriving then gets exclusive access.
/// That is the case a foreign runtime's callbacks actually arrive in, because the
/// guest reaches a host runtime's event loop by suspending into it.
///
/// There is deliberately no way to borrow a store that a frame on this thread is
/// *currently* holding. It would have to be lent first, and an async store cannot
/// be: [`StoreContext::install`](crate::StoreContext) is a no-op for one, since
/// shadowing its entry would hide the write guard from the async host functions
/// that find it by inspecting that entry.
#[cfg(feature = "experimental-async")]
pub struct FunctionEnvHandle<T> {
    store: crate::WeakStoreAsync,
    func_env: FunctionEnv<T>,
}

#[cfg(feature = "experimental-async")]
impl<T> Clone for FunctionEnvHandle<T> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            func_env: self.func_env.clone(),
        }
    }
}

#[cfg(feature = "experimental-async")]
impl<T> Debug for FunctionEnvHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionEnvHandle")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "experimental-async")]
impl<T> FunctionEnvHandle<T> {
    /// The store this environment belongs to, whether or not it is still alive.
    pub fn store_id(&self) -> wasmer_types::StoreId {
        self.store.store_id()
    }

    /// Whether the store still exists. A `false` here is permanent.
    pub fn is_alive(&self) -> bool {
        self.store.upgrade().is_some()
    }
}

#[cfg(feature = "experimental-async")]
impl<T: Any + Send + 'static + Sized> FunctionEnvHandle<T> {
    /// Takes the store's write lock if it is free.
    ///
    /// `None` if the store is gone, or if the lock is held — by a guest that is
    /// running right now, say. Neither is an error; see the type's documentation.
    pub fn try_write(&self) -> Option<FunctionEnvHandleGuard<T>> {
        let store = self.store.upgrade()?;
        Some(FunctionEnvHandleGuard {
            lock: crate::StoreAsyncWriteLock::try_acquire(&store)?,
            func_env: self.func_env.clone(),
        })
    }

    /// Waits for the store's write lock. `None` if the store is already gone.
    pub async fn write(&self) -> Option<FunctionEnvHandleGuard<T>> {
        let store = self.store.upgrade()?;
        Some(FunctionEnvHandleGuard {
            lock: crate::StoreAsyncWriteLock::acquire(&store).await,
            func_env: self.func_env.clone(),
        })
    }
}

/// The store lock a [`FunctionEnvHandle`] acquired, and the environment it
/// reaches through it.
#[cfg(feature = "experimental-async")]
pub struct FunctionEnvHandleGuard<T> {
    lock: crate::StoreAsyncWriteLock,
    func_env: FunctionEnv<T>,
}

#[cfg(feature = "experimental-async")]
impl<T: Any + Send + 'static + Sized> FunctionEnvHandleGuard<T> {
    /// Borrows the environment for as long as this guard is held.
    pub fn as_function_env_mut(&mut self) -> FunctionEnvMut<'_, T> {
        self.func_env.clone().into_mut(&mut self.lock)
    }
}

impl<T> AsStoreRef for FunctionEnvMut<'_, T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.0.as_store_ref()
    }
}

impl<T> AsStoreMut for FunctionEnvMut<'_, T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.0.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        self.0.objects_mut()
    }
}

impl<T> std::fmt::Debug for FunctionEnvMut<'_, T>
where
    T: Send + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A shared handle to a [`FunctionEnv`], suitable for use
/// in async imports.
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvMut<T>(pub(crate) BackendAsyncFunctionEnvMut<T>);

/// A read-only handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandle<T>(pub(crate) BackendAsyncFunctionEnvHandle<T>);

/// A mutable handle to the [`FunctionEnv`] in an [`AsyncFunctionEnvMut`].
#[cfg(feature = "experimental-async")]
pub struct AsyncFunctionEnvHandleMut<T>(pub(crate) BackendAsyncFunctionEnvHandleMut<T>);

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvMut<T> {
    /// Waits for a store lock and returns a read-only handle to the
    /// function environment.
    pub async fn read(&self) -> AsyncFunctionEnvHandle<T> {
        AsyncFunctionEnvHandle(self.0.read().await)
    }

    /// Waits for a store lock and returns a mutable handle to the
    /// function environment.
    pub async fn write(&self) -> AsyncFunctionEnvHandleMut<T> {
        AsyncFunctionEnvHandleMut(self.0.write().await)
    }

    /// Borrows a new immmutable reference
    pub fn as_ref(&self) -> FunctionEnv<T> {
        FunctionEnv(self.0.as_ref())
    }

    /// Borrows a new mutable reference
    pub fn as_mut(&mut self) -> Self {
        Self(self.0.as_mut())
    }

    /// Creates an [`AsStoreAsync`] from this [`AsyncFunctionEnvMut`].
    pub fn as_store_async(&self) -> impl AsStoreAsync + 'static {
        self.0.as_store_async()
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandle<T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        self.0.data()
    }

    /// Returns both the host state and the attached StoreRef
    pub fn data_and_store(&self) -> (&T, &impl AsStoreRef) {
        self.0.data_and_store()
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandle<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.0)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsyncFunctionEnvHandleMut<T> {
    /// Returns a mutable reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }

    /// Returns both the host state and the attached StoreMut
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        self.0.data_and_store_mut()
    }

    /// Borrows a new [`FunctionEnvMut`] from this
    /// [`AsyncFunctionEnvHandleMut`].
    pub fn as_function_env_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut(self.0.as_function_env_mut())
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreRef for AsyncFunctionEnvHandleMut<T> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        AsStoreRef::as_store_ref(&self.0)
    }
}

#[cfg(feature = "experimental-async")]
impl<T: 'static> AsStoreMut for AsyncFunctionEnvHandleMut<T> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        AsStoreMut::as_store_mut(&mut self.0)
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        AsStoreMut::objects_mut(&mut self.0)
    }
}
