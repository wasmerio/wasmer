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
    // We use a box inside the RW lock because the StoreInner shouldn't be moved
    pub(crate) inner: LocalRwLock<Box<StoreInner>>,
    #[cfg(feature = "sys")]
    pub(crate) loan: Option<crate::backend::sys::async_runtime::NativeStoreLoan>,
    #[cfg(feature = "sys")]
    pub(crate) native_invocation:
        Option<std::sync::Arc<crate::backend::sys::async_runtime::NativeInvocationGeneration>>,
}

pub(crate) struct WeakStoreAsync {
    id: StoreId,
    inner: crate::WeakLocalRwLock<Box<StoreInner>>,
    #[cfg(feature = "sys")]
    loan: Option<crate::backend::sys::async_runtime::NativeStoreLoan>,
    #[cfg(feature = "sys")]
    native_invocation:
        Option<std::sync::Arc<crate::backend::sys::async_runtime::NativeInvocationGeneration>>,
}

impl Clone for WeakStoreAsync {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            inner: self.inner.clone(),
            #[cfg(feature = "sys")]
            loan: self.loan.clone(),
            #[cfg(feature = "sys")]
            native_invocation: self.native_invocation.clone(),
        }
    }
}

impl WeakStoreAsync {
    pub(crate) fn upgrade(&self) -> Option<StoreAsync> {
        #[cfg(feature = "sys")]
        if self.loan.as_ref().is_some_and(|loan| !loan.is_live()) {
            return None;
        }
        #[cfg(feature = "sys")]
        if self
            .native_invocation
            .as_ref()
            .is_some_and(|generation| generation.check().is_err())
        {
            return None;
        }
        Some(StoreAsync {
            id: self.id,
            inner: self.inner.upgrade()?,
            #[cfg(feature = "sys")]
            loan: self.loan.clone(),
            #[cfg(feature = "sys")]
            native_invocation: self.native_invocation.clone(),
        })
    }
}

impl StoreAsync {
    pub(crate) fn downgrade(&self) -> WeakStoreAsync {
        WeakStoreAsync {
            id: self.id,
            inner: self.inner.downgrade(),
            #[cfg(feature = "sys")]
            loan: self.loan.clone(),
            #[cfg(feature = "sys")]
            native_invocation: self.native_invocation.clone(),
        }
    }

    pub(crate) fn from_context(
        id: StoreId,
        #[cfg(feature = "sys")] native_invocation: Option<
            std::sync::Arc<crate::backend::sys::async_runtime::NativeInvocationGeneration>,
        >,
    ) -> Option<Self> {
        #[cfg(feature = "sys")]
        if let Some(loan) = crate::backend::sys::async_runtime::current_store_loan(id) {
            return Some(loan);
        }
        // Safety: we don't keep the guard around, it's just used to
        // build a safe lock handle.
        match unsafe { StoreContext::try_get_current_async(id) } {
            crate::GetStoreAsyncGuardResult::Ok(guard) => Some(Self {
                id,
                inner: guard.lock_handle(),
                #[cfg(feature = "sys")]
                loan: None,
                #[cfg(feature = "sys")]
                native_invocation,
            }),
            _ => None,
        }
    }

    /// Transform this [`StoreAsync`] back into a [`Store`]
    /// if this is the only clone of it and is unlocked.
    pub fn into_store(self) -> Result<Store, Self> {
        #[cfg(feature = "sys")]
        if self.loan.is_some() {
            return Err(self);
        }
        #[cfg(feature = "sys")]
        if self.native_invocation.as_ref().is_some_and(|generation| {
            generation
                .superseded
                .load(std::sync::atomic::Ordering::Acquire)
        }) {
            return Err(self);
        }
        #[cfg(feature = "sys")]
        let native_invocation = self.native_invocation;
        match self.inner.consume() {
            Ok(unwrapped) => Ok(Store { inner: unwrapped }),
            Err(lock) => Err(Self {
                id: self.id,
                inner: lock,
                #[cfg(feature = "sys")]
                loan: None,
                #[cfg(feature = "sys")]
                native_invocation,
            }),
        }
    }

    /// Acquire a read lock on the store. Panics if the store is
    /// locked for writing.
    pub fn read(&self) -> StoreAsyncReadLock {
        if !StoreContext::is_empty() {
            panic!("This method cannot be called from inside imported functions");
        }

        StoreAsyncReadLock::try_acquire(self).expect("StoreAsync is locked or its loan expired")
    }

    /// Acquire a write lock on the store. Panics if the store is
    /// locked.
    pub fn write(self) -> StoreAsyncWriteLock {
        if !StoreContext::is_empty() {
            panic!("This method cannot be called from inside imported functions");
        }

        StoreAsyncWriteLock::try_acquire(&self).expect("StoreAsync is locked or its loan expired")
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
            #[cfg(feature = "sys")]
            loan: store.loan.clone(),
            #[cfg(feature = "sys")]
            native_invocation: store.native_invocation.clone(),
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
    inner: StoreReadGuard,
}

enum StoreReadGuard {
    Owned(LocalRwLockReadGuard<Box<StoreInner>>),
    #[cfg(feature = "sys")]
    Loan(crate::backend::sys::async_runtime::NativeLoanReadGuard),
}

impl StoreAsyncReadLock {
    pub(crate) async fn acquire(store: &StoreAsync) -> Self {
        #[cfg(feature = "sys")]
        if let Some(loan) = &store.loan {
            return Self {
                inner: StoreReadGuard::Loan(loan.read().await),
            };
        }
        let store_ref = store.inner.read().await;
        Self {
            inner: StoreReadGuard::Owned(store_ref),
        }
    }

    pub(crate) fn try_acquire(store: &StoreAsync) -> Option<Self> {
        #[cfg(feature = "sys")]
        if let Some(loan) = &store.loan {
            return loan.try_read().map(|inner| Self {
                inner: StoreReadGuard::Loan(inner),
            });
        }
        store.inner.try_read().map(|inner| Self {
            inner: StoreReadGuard::Owned(inner),
        })
    }
}

impl AsStoreRef for StoreAsyncReadLock {
    fn as_store_ref(&self) -> StoreRef<'_> {
        let inner = match &self.inner {
            StoreReadGuard::Owned(guard) => &***guard,
            #[cfg(feature = "sys")]
            StoreReadGuard::Loan(guard) => guard.store(),
        };
        StoreRef { inner }
    }
}

/// A write lock on an async store.
pub struct StoreAsyncWriteLock {
    inner: StoreWriteGuard,
}

enum StoreWriteGuard {
    Owned(OwnedStoreWriteGuard),
    #[cfg(feature = "sys")]
    Loan(crate::backend::sys::async_runtime::NativeLoanWriteGuard),
}

// This wrapper clears origin before releasing the real lock, including unwind.
// It adds no Store access beyond that lock and performs no callbacks while held.
struct OwnedStoreWriteGuard {
    guard: Option<LocalRwLockWriteGuard<Box<StoreInner>>>,
    #[cfg(feature = "sys")]
    origin: Option<std::sync::Arc<crate::backend::sys::async_runtime::NativeInvocationGeneration>>,
}
impl OwnedStoreWriteGuard {
    fn new(
        guard: LocalRwLockWriteGuard<Box<StoreInner>>,
        #[cfg(feature = "sys")] origin: Option<
            std::sync::Arc<crate::backend::sys::async_runtime::NativeInvocationGeneration>,
        >,
    ) -> Self {
        #[cfg(feature = "sys")]
        let guard = {
            let mut guard = guard;
            assert!(guard.native_access_origin.is_none());
            guard.native_access_origin = origin.clone();
            guard
        };
        // Keep the ordinary JS path's parameter used without changing its data.
        let guard = Some(guard);
        Self {
            guard,
            #[cfg(feature = "sys")]
            origin,
        }
    }
    fn store(&self) -> &StoreInner {
        self.guard.as_ref().unwrap()
    }
    fn store_mut(&mut self) -> &mut StoreInner {
        self.guard.as_mut().unwrap()
    }
    fn clear_origin(&mut self) {
        #[cfg(feature = "sys")]
        if let Some(guard) = self.guard.as_mut() {
            assert!(match (&guard.native_access_origin, &self.origin) {
                (Some(current), Some(expected)) => std::sync::Arc::ptr_eq(current, expected),
                (None, None) => true,
                _ => false,
            });
            guard.native_access_origin = None;
            self.origin = None;
        }
    }
    #[cfg(feature = "js")]
    fn into_inner(mut self) -> LocalRwLockWriteGuard<Box<StoreInner>> {
        self.clear_origin();
        self.guard.take().unwrap()
    }
}
impl Drop for OwnedStoreWriteGuard {
    fn drop(&mut self) {
        self.clear_origin();
    }
}

impl StoreAsyncWriteLock {
    pub(crate) async fn acquire(store: &StoreAsync) -> Self {
        #[cfg(feature = "sys")]
        if let Some(loan) = &store.loan {
            return Self {
                inner: StoreWriteGuard::Loan(loan.write().await),
            };
        }
        let store_guard = store.inner.write().await;
        Self {
            inner: StoreWriteGuard::Owned(OwnedStoreWriteGuard::new(
                store_guard,
                #[cfg(feature = "sys")]
                store.native_invocation.clone(),
            )),
        }
    }

    pub(crate) fn try_acquire(store: &StoreAsync) -> Option<Self> {
        #[cfg(feature = "sys")]
        if let Some(loan) = &store.loan {
            return loan.try_write().map(|inner| Self {
                inner: StoreWriteGuard::Loan(inner),
            });
        }
        store.inner.try_write().map(|inner| Self {
            inner: StoreWriteGuard::Owned(OwnedStoreWriteGuard::new(
                inner,
                #[cfg(feature = "sys")]
                store.native_invocation.clone(),
            )),
        })
    }

    #[cfg(feature = "js")]
    pub(crate) fn into_owned(self) -> LocalRwLockWriteGuard<Box<StoreInner>> {
        match self.inner {
            StoreWriteGuard::Owned(guard) => guard.into_inner(),
            #[cfg(feature = "sys")]
            StoreWriteGuard::Loan(_) => panic!("native Store loan cannot enter a JS context"),
        }
    }
}

impl AsStoreRef for StoreAsyncWriteLock {
    fn as_store_ref(&self) -> StoreRef<'_> {
        let inner = match &self.inner {
            StoreWriteGuard::Owned(guard) => guard.store(),
            #[cfg(feature = "sys")]
            StoreWriteGuard::Loan(guard) => guard.store(),
        };
        StoreRef { inner }
    }
}

impl AsStoreMut for StoreAsyncWriteLock {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        let inner = match &mut self.inner {
            StoreWriteGuard::Owned(guard) => guard.store_mut(),
            #[cfg(feature = "sys")]
            StoreWriteGuard::Loan(guard) => guard.store_mut(),
        };
        StoreMut { inner }
    }

    fn objects_mut(&mut self) -> &mut super::StoreObjects {
        &mut self.as_store_mut().inner.objects
    }
}
