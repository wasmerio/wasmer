use std::{fmt::Debug, marker::PhantomData};

use crate::{
    AsStoreMut, AsStoreRef, LocalRwLock, LocalRwLockReadGuard, LocalRwLockWriteGuard, Store,
    StoreContext, StoreInner, StoreMut, StorePtrWrapper, StoreRef,
};

use wasmer_types::StoreId;

/// A store that can be used to invoke
/// [`Function::call_async`](crate::Function::call_async).
pub struct StoreAsync {
    pub(crate) id: StoreId,
    pub(crate) inner: LocalRwLock<StoreInner>,
}

impl StoreAsync {
    pub(crate) fn from_context(id: StoreId) -> Option<Self> {
        // Safety: we don't keep the guard around, it's just used to
        // build a safe lock handle.
        match unsafe { StoreContext::try_get_current_async(id) } {
            crate::GetStoreAsyncGuardResult::Ok(guard) => Some(Self {
                id,
                inner: crate::LocalRwLockWriteGuard::lock_handle(unsafe {
                    guard.guard.as_ref().unwrap()
                }),
            }),
            _ => None,
        }
    }

    /// Transform this [`StoreAsync`] back into a [`Store`]
    /// if this is the only clone of it and is unlocked.
    pub fn into_store(self) -> Result<Store, Self> {
        match self.inner.consume() {
            Ok(unwrapped) => Ok(Store {
                inner: Box::new(unwrapped),
            }),
            Err(lock) => Err(Self {
                id: self.id,
                inner: lock,
            }),
        }
    }

    /// Acquire a read lock on the store. Panics if the store is
    /// locked for writing.
    pub fn read(&self) -> StoreAsyncReadLock {
        if !StoreContext::is_empty() {
            panic!("This method cannot be called from inside imported functions");
        }

        let store_ref = self
            .inner
            .try_read()
            .expect("StoreAsync is locked for write");
        StoreAsyncReadLock { inner: store_ref }
    }

    /// Acquire a write lock on the store. Panics if the store is
    /// locked.
    pub fn write(self) -> StoreAsyncWriteLock {
        if !StoreContext::is_empty() {
            panic!("This method cannot be called from inside imported functions");
        }

        let store_guard = self.inner.try_write().expect("StoreAsync is locked");
        StoreAsyncWriteLock { inner: store_guard }
    }
}

impl Debug for StoreAsync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreAsync").field("id", &self.id).finish()
    }
}

/// A trait for types that can be used with
/// [`Function::call_async`](crate::Function::call_async).
pub trait AsStoreAsync {
    /// Returns a reference to the inner store.
    fn store_ref(&self) -> &StoreAsync;

    /// Returns a copy of the store.
    fn store(&self) -> StoreAsync {
        let store = self.store_ref();
        StoreAsync {
            id: store.id,
            inner: store.inner.clone(),
        }
    }

    /// Returns the store id.
    fn store_id(&self) -> StoreId {
        self.store().id
    }

    /// Acquires a read lock on the store.
    fn read_lock(&self) -> impl Future<Output = StoreAsyncReadLock> {
        StoreAsyncReadLock::acquire(self.store_ref())
    }

    /// Acquires a write lock on the store.
    fn write_lock(&self) -> impl Future<Output = StoreAsyncWriteLock> {
        StoreAsyncWriteLock::acquire(self.store_ref())
    }
}

impl AsStoreAsync for StoreAsync {
    fn store_ref(&self) -> &StoreAsync {
        self
    }
}

/// A read lock on an async store.
pub struct StoreAsyncReadLock {
    pub(crate) inner: LocalRwLockReadGuard<StoreInner>,
}

impl StoreAsyncReadLock {
    pub(crate) async fn acquire(store: &StoreAsync) -> Self {
        let store_ref = store.inner.read().await;
        Self { inner: store_ref }
    }
}

impl AsStoreRef for StoreAsyncReadLock {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: &self.inner }
    }
}

/// A write lock on an async store.
pub struct StoreAsyncWriteLock {
    pub(crate) inner: LocalRwLockWriteGuard<StoreInner>,
}

impl StoreAsyncWriteLock {
    pub(crate) async fn acquire(store: &StoreAsync) -> Self {
        let store_guard = store.inner.write().await;
        Self { inner: store_guard }
    }
}

impl AsStoreRef for StoreAsyncWriteLock {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: &self.inner }
    }
}

impl AsStoreMut for StoreAsyncWriteLock {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut {
            inner: &mut self.inner,
        }
    }

    fn objects_mut(&mut self) -> &mut super::StoreObjects {
        &mut self.inner.objects
    }
}
