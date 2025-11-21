use std::marker::PhantomData;

use crate::{AsStoreMut, AsStoreRef, Store, StoreContext, StoreMut, StoreMutWrapper, StoreRef};

use wasmer_types::StoreId;

/// A trait for types that can be used with
/// [`Function::call_async`](crate::Function::call_async).
///
/// Note that, while this trait can easily be implemented for a lot
/// of types (including [`StoreMut`]), implementations are left
/// out on purpose to help avoid common deadlock scenarios.
pub trait AsAsyncStore {
    /// Returns a reference to the inner store.
    fn store_ref(&self) -> &Store;

    /// Returns a copy of the store.
    fn store(&self) -> Store {
        let store = self.store_ref();
        Store {
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
    fn write_lock<'a>(
        &'a self,
    ) -> impl Future<Output = AsyncStoreWriteLock<'a>> + 'a {
        AsyncStoreWriteLock::acquire(self.store_ref())
    }
}

impl AsAsyncStore for Store {
    fn store_ref(&self) -> &Store {
        self
    }
}
pub(crate) enum AsyncStoreReadLockInner {
    Owned(StoreRef),
    FromStoreContext(StoreMutWrapper),
}

/// A read lock on a store that can be used in concurrent contexts;
/// mostly useful in conjunction with [`AsAsyncStore`].
pub struct AsyncStoreReadLock<'a> {
    pub(crate) inner: AsyncStoreReadLockInner,
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncStoreReadLock<'a> {
    pub(crate) async fn acquire(store: &'a Store) -> Self {
        let store_context = unsafe { StoreContext::try_get_current(store.id) };
        match store_context {
            Some(store_mut_wrapper) => Self {
                inner: AsyncStoreReadLockInner::FromStoreContext(store_mut_wrapper),
                _marker: PhantomData,
            },
            None => {
                // Drop the option before awaiting, since the value isn't Send
                drop(store_context);
                let store_ref = store.make_ref_async().await;
                Self {
                    inner: AsyncStoreReadLockInner::Owned(store_ref),
                    _marker: PhantomData,
                }
            }
        }
    }
}

impl AsStoreRef for AsyncStoreReadLock<'_> {
    fn as_ref(&self) -> &crate::StoreInner {
        match &self.inner {
            AsyncStoreReadLockInner::Owned(guard) => guard.as_ref(),
            AsyncStoreReadLockInner::FromStoreContext(wrapper) => wrapper.as_ref().as_ref(),
        }
    }
}

pub(crate) enum AsyncStoreWriteLockInner {
    Owned(StoreMut),
    FromStoreContext(StoreMutWrapper),
}

/// A write lock on a store that can be used in concurrent contexts;
/// mostly useful in conjunction with [`AsAsyncStore`].
pub struct AsyncStoreWriteLock<'a> {
    pub(crate) inner: AsyncStoreWriteLockInner,
    _marker: PhantomData<&'a ()>,
}

impl<'a> AsyncStoreWriteLock<'a> {
    pub(crate) async fn acquire(store: &'a Store) -> Self {
        let store_context = unsafe { StoreContext::try_get_current(store.id) };
        match store_context {
            Some(store_mut_wrapper) => Self {
                inner: AsyncStoreWriteLockInner::FromStoreContext(store_mut_wrapper),
                _marker: PhantomData,
            },
            None => {
                // Drop the option before awaiting, since the value isn't Send
                drop(store_context);
                let store_mut = store.make_mut_async().await;
                Self {
                    inner: AsyncStoreWriteLockInner::Owned(store_mut),
                    _marker: PhantomData,
                }
            }
        }
    }
}

impl AsStoreRef for AsyncStoreWriteLock<'_> {
    fn as_ref(&self) -> &crate::StoreInner {
        match &self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => guard.as_ref(),
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_ref().as_ref(),
        }
    }
}

impl AsStoreMut for AsyncStoreWriteLock<'_> {
    fn as_mut(&mut self) -> &mut crate::StoreInner {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => guard.as_mut(),
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_mut().as_mut(),
        }
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => guard.reborrow_mut(),
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_mut(),
        }
    }

    fn take(&mut self) -> Option<StoreMut> {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => guard.take(),
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => wrapper.as_mut().take(),
        }
    }

    fn put_back(&mut self, store_mut: StoreMut) {
        match &mut self.inner {
            AsyncStoreWriteLockInner::Owned(guard) => guard.put_back(store_mut),
            AsyncStoreWriteLockInner::FromStoreContext(wrapper) => {
                wrapper.as_mut().put_back(store_mut)
            }
        }
    }
}
