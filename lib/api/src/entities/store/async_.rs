use std::marker::PhantomData;

use crate::{
    AsStoreMut, AsStoreRef, LocalReadGuardRc, LocalRwLock, LocalWriteGuardRc, Store, StoreContext,
    StoreInner, StoreMut, StorePtrWrapper, StoreRef,
};

use wasmer_types::StoreId;

/// A store that can be used to invoke
/// [`Function::call_async`](crate::Function::call_async).
pub struct StoreAsync {
    pub(crate) id: StoreId,
    pub(crate) inner: LocalRwLock<StoreInner>,
}

impl StoreAsync {
    /// Transform this [`StoreAsync`] back into a [`Store`]
    /// if there are no coroutines running or waiting to run
    /// against it.
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
}

/// A trait for types that can be used with
/// [`Function::call_async`](crate::Function::call_async).
///
/// Note that, while this trait can easily be implemented for a lot
/// of types (including [`StoreMut`]), implementations are left
/// out on purpose to help avoid common deadlock scenarios.
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
    fn read_lock<'a>(&'a self) -> impl Future<Output = AsyncStoreReadLock<'a>> + 'a {
        AsyncStoreReadLock::acquire(self.store_ref())
    }

    /// Acquires a write lock on the store.
    fn write_lock<'a>(&'a self) -> impl Future<Output = AsyncStoreWriteLock<'a>> + 'a {
        AsyncStoreWriteLock::acquire(self.store_ref())
    }
}

impl AsStoreAsync for StoreAsync {
    fn store_ref(&self) -> &StoreAsync {
        self
    }
}

pub(crate) enum AsyncStoreReadLockInner {
    Owned(LocalReadGuardRc<StoreInner>),
    FromStoreContext(StorePtrWrapper),
}

/// A read lock on a store that can be used in concurrent contexts;
/// mostly useful in conjunction with [`AsStoreAsync`].
pub struct AsyncStoreReadLock<'a> {
    pub(crate) inner: AsyncStoreReadLockInner,
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncStoreReadLock<'a> {
    pub(crate) async fn acquire(store: &'a StoreAsync) -> Self {
        let store_context = unsafe { StoreContext::try_get_current(store.id) };
        match store_context {
            Some(store_mut_wrapper) => Self {
                inner: AsyncStoreReadLockInner::FromStoreContext(store_mut_wrapper),
                _marker: PhantomData,
            },
            None => {
                // Drop the option before awaiting, since the value isn't Send
                drop(store_context);
                let store_ref = store.inner.read_rc().await;
                Self {
                    inner: AsyncStoreReadLockInner::Owned(store_ref),
                    _marker: PhantomData,
                }
            }
        }
    }
}

impl AsStoreRef for AsyncStoreReadLock<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match &self.inner {
            AsyncStoreReadLockInner::Owned(guard) => StoreRef { inner: &*guard },
            AsyncStoreReadLockInner::FromStoreContext(wrapper) => wrapper.as_ref(),
        }
    }
}

pub(crate) enum AsyncStoreWriteLockInner {
    Owned(LocalWriteGuardRc<StoreInner>),
    FromStoreContext(StorePtrWrapper),
}

/// A write lock on a store that can be used in concurrent contexts;
/// mostly useful in conjunction with [`AsStoreAsync`].
pub struct AsyncStoreWriteLock<'a> {
    pub(crate) inner: AsyncStoreWriteLockInner,
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncStoreWriteLock<'a> {
    pub(crate) async fn acquire(store: &'a StoreAsync) -> Self {
        let store_context = unsafe { StoreContext::try_get_current(store.id) };
        match store_context {
            Some(store_mut_wrapper) => Self {
                inner: AsyncStoreWriteLockInner::FromStoreContext(store_mut_wrapper),
                _marker: PhantomData,
            },
            None => {
                // Drop the option before awaiting, since the value isn't Send
                drop(store_context);
                let store_guard = store.inner.write_rc().await;
                Self {
                    inner: AsyncStoreWriteLockInner::Owned(store_guard),
                    _marker: PhantomData,
                }
            }
        }
    }
}

impl AsStoreRef for AsyncStoreWriteLock<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        match &self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => StoreRef { inner: &*guard },
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_ref(),
        }
    }
}

impl AsStoreMut for AsyncStoreWriteLock<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => StoreMut { inner: &mut *guard },
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_mut(),
        }
    }

    fn objects_mut(&mut self) -> &mut super::StoreObjects {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => &mut guard.objects,
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => {
                &mut wrapper.as_mut().inner.objects
            }
        }
    }
}
