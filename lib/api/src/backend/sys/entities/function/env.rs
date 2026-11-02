use std::{any::Any, fmt::Debug, marker::PhantomData};

#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync, StoreAsyncReadLock, StoreAsyncWriteLock};
use crate::{
    Store, StoreContext, StoreInner, StoreMut, StorePtrWrapper,
    store::{AsStoreMut, AsStoreRef, StoreRef},
};

use wasmer_vm::{
    StoreHandle, StoreId, StoreObject, StoreObjects, VMFunctionEnvironment,
    VMFunctionEnvironmentLease,
};

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
            split_lease: None,
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
    pub(crate) split_lease: Option<SplitEnvironmentLease<'a>>,
}

pub(crate) enum SplitEnvironmentLease<'a> {
    Owned(VMFunctionEnvironmentLease),
    Borrowed(&'a mut VMFunctionEnvironmentLease),
}

impl SplitEnvironmentLease<'_> {
    fn lease(&self) -> &VMFunctionEnvironmentLease {
        match self {
            Self::Owned(lease) => lease,
            Self::Borrowed(lease) => lease,
        }
    }

    fn lease_mut(&mut self) -> &mut VMFunctionEnvironmentLease {
        match self {
            Self::Owned(lease) => lease,
            Self::Borrowed(lease) => lease,
        }
    }
}

impl<T> Debug for FunctionEnvMut<'_, T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data().fmt(f)
    }
}

impl<T: Send + 'static> FunctionEnvMut<'_, T> {
    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        match &self.split_lease {
            Some(lease) => lease.lease().as_ref().downcast_ref().unwrap(),
            None => self.func_env.as_ref(&self.store_mut),
        }
    }

    /// Returns a mutable- reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        match &mut self.split_lease {
            Some(lease) => lease.lease_mut().as_mut().downcast_mut().unwrap(),
            None => self.func_env.as_mut(&mut self.store_mut),
        }
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
            split_lease: self
                .split_lease
                .as_mut()
                .map(|lease| SplitEnvironmentLease::Borrowed(lease.lease_mut())),
        }
    }

    /// Borrows a new mutable reference of both the attached Store and host state
    /// The data remains lent to this temporary handle until it is dropped;
    /// other handles to the same environment reject access during that time.
    pub fn data_and_store_mut(&mut self) -> (&mut T, StoreMut<'_>) {
        if self.split_lease.is_none() {
            let lease = self
                .func_env
                .handle
                .get_mut(self.store_mut.objects_mut().as_sys_mut())
                .lend();
            self.split_lease = Some(SplitEnvironmentLease::Owned(lease));
        }
        let data = self
            .split_lease
            .as_mut()
            .unwrap()
            .lease_mut()
            .as_mut()
            .downcast_mut()
            .unwrap();
        (data, self.store_mut.as_store_mut())
    }

    /// Returns a [`StoreAsync`] if the current
    /// context is asynchronous. The store will be locked since
    /// it's already active in the current context, but can be used
    /// to spawn new coroutines via
    /// [`Function::call_async`](crate::Function::call_async).
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
    split_lease: Option<VMFunctionEnvironmentLease>,
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
            split_lease: None,
        }
    }

    /// A read-only handle to the function environment if the store lock
    /// is free for reading now, without waiting.
    pub fn try_read(&self) -> Option<AsyncFunctionEnvHandle<T>> {
        let read_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => StoreAsyncReadLock::try_acquire(store)?,
            // We can never acquire a store lock in a sync context
            AsyncFunctionEnvMutStore::Sync(_) => return None,
        };
        Some(AsyncFunctionEnvHandle {
            read_lock,
            func_env: self.func_env.clone(),
        })
    }

    /// A mutable handle to the function environment if the store lock is
    /// free now, without waiting.
    pub fn try_write(&self) -> Option<AsyncFunctionEnvHandleMut<T>> {
        let write_lock = match &self.store {
            AsyncFunctionEnvMutStore::Async(store) => StoreAsyncWriteLock::try_acquire(store)?,
            // We can never acquire a store lock in a sync context
            AsyncFunctionEnvMutStore::Sync(_) => return None,
        };
        Some(AsyncFunctionEnvHandleMut {
            write_lock,
            func_env: self.func_env.clone(),
            split_lease: None,
        })
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
        match &mut self.split_lease {
            Some(lease) => lease.as_mut().downcast_mut().unwrap(),
            None => self.func_env.as_mut(&mut self.write_lock),
        }
    }

    /// Returns both the host state and the attached StoreMut
    /// The data remains lent to this write handle until it is dropped.
    pub fn data_and_store_mut(&mut self) -> (&mut T, &mut impl AsStoreMut) {
        if self.split_lease.is_none() {
            self.split_lease = Some(
                self.func_env
                    .handle
                    .get_mut(self.write_lock.objects_mut().as_sys_mut())
                    .lend(),
            );
        }
        let data = self
            .split_lease
            .as_mut()
            .unwrap()
            .as_mut()
            .downcast_mut()
            .unwrap();
        (data, &mut self.write_lock)
    }

    /// Borrows a new [`FunctionEnvMut`] from this [`AsyncFunctionEnvHandleMut`].
    pub fn as_function_env_mut(&mut self) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut {
            store_mut: self.write_lock.as_store_mut(),
            func_env: self.func_env.clone(),
            split_lease: self
                .split_lease
                .as_mut()
                .map(SplitEnvironmentLease::Borrowed),
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

#[cfg(test)]
mod split_ownership {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct State {
        value: i32,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for State {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_split_keeps_its_exact_data_when_the_store_slot_is_removed_and_replaced() {
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let env = FunctionEnv::new(
            &mut store,
            State {
                value: 7,
                drops: drops.clone(),
            },
        );
        let mut call = env.into_mut(&mut store);
        {
            let (data, mut store) = call.data_and_store_mut();
            let mut removed = VMFunctionEnvironment::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            let read_rejected = catch_unwind(AssertUnwindSafe(|| {
                let _ = removed.as_ref();
            }))
            .is_err();
            let write_rejected = catch_unwind(AssertUnwindSafe(|| {
                let _ = removed.as_mut();
            }))
            .is_err();
            let second_lease_rejected = catch_unwind(AssertUnwindSafe(|| removed.lend())).is_err();
            let take_rejected = catch_unwind(AssertUnwindSafe(|| removed.into_contents())).is_err();
            VMFunctionEnvironment::list_mut(store.objects_mut().as_sys_mut())
                .push(VMFunctionEnvironment::new(99_i32));
            data.value += 35;
            assert!(read_rejected && write_rejected && second_lease_rejected && take_rejected);
            assert_eq!(drops.load(Ordering::SeqCst), 0);
        }
        // Access through this owning handle follows its lease, not the new slot.
        assert_eq!(call.data().value, 42);
        {
            let mut child = call.as_mut();
            child.data_mut().value += 1;
            assert_eq!(child.data_and_store_mut().0.value, 43);
        }
        assert_eq!(call.data().value, 43);
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[cfg(feature = "experimental-async")]
    fn an_async_write_handle_retains_and_reborrows_its_split_data() {
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let env = FunctionEnv::new(
            &mut store,
            State {
                value: 7,
                drops: drops.clone(),
            },
        );
        let env = AsyncFunctionEnvMut {
            store: AsyncFunctionEnvMutStore::Async(store.into_async()),
            func_env: env,
        };
        let mut write = futures::executor::block_on(env.write());
        {
            let (data, store) = write.data_and_store_mut();
            let removed = VMFunctionEnvironment::list_mut(store.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            drop(removed);
            data.value += 35;
            assert_eq!(drops.load(Ordering::SeqCst), 0);
        }
        {
            let mut child = write.as_function_env_mut();
            assert_eq!(child.data().value, 42);
            child.data_and_store_mut().0.value += 1;
        }
        assert_eq!(write.data_mut().value, 43);
        drop(write);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_split_lease_can_move_threads_without_exposing_its_data_through_the_owner() {
        let mut env = VMFunctionEnvironment::new(7_i32);
        let mut lease = env.lend();
        let (ready, started) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            *lease.as_mut().downcast_mut::<i32>().unwrap() += 35;
            ready.send(()).unwrap();
            wait.recv().unwrap();
            drop(lease);
        });
        started.recv().unwrap();
        let rejected = catch_unwind(AssertUnwindSafe(|| {
            let _ = env.as_ref();
        }))
        .is_err();
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(rejected);
        assert_eq!(*env.as_ref().downcast_ref::<i32>().unwrap(), 42);
    }
}
