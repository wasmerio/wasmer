use std::{any::Any, fmt::Debug, marker::PhantomData};

#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync, StoreAsyncReadLock, StoreAsyncWriteLock};
use crate::{
    Store, StoreContext, StoreInner, StoreMut, StorePtrWrapper,
    store::{AsStoreMut, AsStoreRef, StoreRef},
};

use wasmer_vm::{
    StoreHandle, StoreId, StoreObject, StoreObjects, VMFunctionEnvironment,
    VMFunctionEnvironmentFramePin, VMFunctionEnvironmentIdentity, VMFunctionEnvironmentLease,
};

#[derive(Debug)]
/// An opaque reference to a function environment.
/// The function environment data is owned by the `Store`.
pub struct FunctionEnv<T> {
    pub(crate) handle: StoreHandle<VMFunctionEnvironment>,
    identity: VMFunctionEnvironmentIdentity,
    marker: PhantomData<T>,
}

impl<T: Any + Send + 'static + Sized> FunctionEnv<T> {
    /// Make a new FunctionEnv
    pub fn new(store: &mut impl AsStoreMut, value: T) -> Self {
        let owner = VMFunctionEnvironment::new(value);
        let identity = owner.identity();
        Self {
            handle: StoreHandle::new(store.objects_mut().as_sys_mut(), owner),
            identity,
            marker: PhantomData,
        }
    }

    /// Convert an attached exact environment into a temporary pinned frame.
    pub fn into_mut(self, store: &mut impl AsStoreMut) -> FunctionEnvMut<'_, T> {
        FunctionEnvMut::new(store.as_store_mut(), self)
    }
}

impl<T: Any + 'static + Sized> FunctionEnv<T> {
    /// Get the data as reference
    pub fn as_ref<'a>(&self, store: &'a impl AsStoreRef) -> &'a T {
        let view = store.as_store_ref();
        let key = view.inner as *const StoreInner as usize;
        let owner = self
            .identity
            .lookup(&self.handle, view.objects().as_sys())
            .expect("function environment is detached or replaced");
        // SAFETY: exact identity in this actual shared Store borrow was checked.
        unsafe { owner.borrow_for_store(key) }
            .downcast_ref::<T>()
            .unwrap()
    }

    pub fn as_mut<'a>(&self, store: &'a mut impl AsStoreMut) -> &'a mut T {
        let view = store.as_store_mut();
        let key = view.inner as *const StoreInner as usize;
        let owner = self
            .identity
            .lookup(&self.handle, view.inner.objects.as_sys())
            .expect("function environment is detached or replaced");
        // SAFETY: exclusive actual Store borrow and exact allocation identity.
        unsafe { &mut *owner.borrow_mut_ptr_for_store(key) }
            .downcast_mut::<T>()
            .unwrap()
    }

    fn pin_for_store(&self, store: &impl AsStoreRef) -> Option<VMFunctionEnvironmentFramePin> {
        let view = store.as_store_ref();
        let key = view.inner as *const StoreInner as usize;
        let owner = self
            .identity
            .lookup(&self.handle, view.objects().as_sys())?;
        // SAFETY: allocation is attached to this exact actual Store borrow.
        unsafe { owner.pin_for_store(key) }
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
        self.handle == other.handle && self.identity == other.identity
    }
}

impl<T> Eq for FunctionEnv<T> {}

impl<T> std::hash::Hash for FunctionEnv<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.handle.hash(state);
        self.identity.hash(state);
        self.marker.hash(state);
    }
}

impl<T> Clone for FunctionEnv<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            identity: self.identity.clone(),
            marker: self.marker,
        }
    }
}

/// A temporary handle to a [`FunctionEnv`].
pub struct FunctionEnvMut<'a, T: 'a> {
    // Exact T cleanup must finish before releasing the final allocation pin.
    pub(crate) split_lease: Option<SplitEnvironmentLease<'a>>,
    frame: EnvironmentFrame<'a>,
    pub(crate) store_mut: StoreMut<'a>,
    pub(crate) func_env: FunctionEnv<T>,
}

enum EnvironmentFrame<'a> {
    Owned(VMFunctionEnvironmentFramePin),
    Borrowed(&'a VMFunctionEnvironmentFramePin),
}
impl EnvironmentFrame<'_> {
    fn pin(&self) -> &VMFunctionEnvironmentFramePin {
        match self {
            Self::Owned(pin) => pin,
            Self::Borrowed(pin) => pin,
        }
    }
}

/// A temporary native environment allocation, without independent data access.
#[doc(hidden)]
pub struct FunctionEnvFramePin<T> {
    frame: VMFunctionEnvironmentFramePin,
    func_env: FunctionEnv<T>,
}
impl<T: Send + 'static> FunctionEnvFramePin<T> {
    /// Reborrow only through an attached exact cell and the actual Store.
    pub fn try_as_env_mut<'a>(
        &'a self,
        store: &'a mut impl AsStoreMut,
    ) -> Option<crate::FunctionEnvMut<'a, T>> {
        let store_mut = store.as_store_mut();
        if !matches!(&store_mut.inner.store, crate::BackendStore::Sys(_)) {
            return None;
        }
        let key = store_mut.inner as *const StoreInner as usize;
        self.func_env.identity.lookup(
            &self.func_env.handle,
            store_mut.as_store_ref().objects().as_sys(),
        )?;
        if !self.frame.can_reborrow(key) {
            return None;
        }
        Some(
            FunctionEnvMut {
                split_lease: None,
                frame: EnvironmentFrame::Borrowed(&self.frame),
                store_mut,
                func_env: self.func_env.clone(),
            }
            .into(),
        )
    }
}

impl<T: Send + 'static> crate::FunctionEnvMut<'_, T> {
    /// Retain allocation custody for a native import or callback frame.
    #[doc(hidden)]
    pub fn pin_frame(&self) -> Option<FunctionEnvFramePin<T>> {
        match &self.0 {
            crate::BackendFunctionEnvMut::Sys(env) => Some(env.pin_frame()),
            #[cfg(feature = "js")]
            crate::BackendFunctionEnvMut::Js(_) => None,
            #[cfg(feature = "v8")]
            crate::BackendFunctionEnvMut::V8(_) => None,
        }
    }
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

impl<'a, T: Send + 'static> FunctionEnvMut<'a, T> {
    pub(crate) fn new(store_mut: StoreMut<'a>, func_env: FunctionEnv<T>) -> Self {
        let pin = func_env
            .pin_for_store(&store_mut)
            .expect("function environment is detached or on another Store/thread");
        Self {
            split_lease: None,
            frame: EnvironmentFrame::Owned(pin),
            store_mut,
            func_env,
        }
    }

    pub fn pin_frame(&self) -> FunctionEnvFramePin<T> {
        let key = self.store_mut.inner as *const StoreInner as usize;
        // SAFETY: this temporary frame still holds its original Store borrow.
        let frame = unsafe { self.frame.pin().duplicate(key) };
        FunctionEnvFramePin {
            frame,
            func_env: self.func_env.clone(),
        }
    }

    /// Returns a reference to the host state in this function environment.
    pub fn data(&self) -> &T {
        match &self.split_lease {
            Some(lease) => lease.lease().as_ref().downcast_ref().unwrap(),
            None => {
                let key = self.store_mut.inner as *const StoreInner as usize;
                // SAFETY: original frame pin and actual Store borrow remain held.
                unsafe { self.frame.pin().borrow_for_store(key) }
                    .downcast_ref()
                    .unwrap()
            }
        }
    }

    /// Returns a mutable- reference to the host state in this function environment.
    pub fn data_mut(&mut self) -> &mut T {
        match &mut self.split_lease {
            Some(lease) => lease.lease_mut().as_mut().downcast_mut().unwrap(),
            None => {
                let key = self.store_mut.inner as *const StoreInner as usize;
                // SAFETY: exclusive actual Store borrow, same original frame cell.
                unsafe { &mut *self.frame.pin().borrow_mut_ptr_for_store(key) }
                    .downcast_mut()
                    .unwrap()
            }
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
            frame: EnvironmentFrame::Borrowed(self.frame.pin()),
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
            let key = self.store_mut.inner as *const StoreInner as usize;
            // SAFETY: same original frame and exclusive Store borrow; no data
            // reference is live before this new unique split is installed.
            let lease = unsafe { self.frame.pin().lend_for_store(key) };
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

    #[cfg(feature = "experimental-async")]
    pub(crate) fn checked_async(&self) -> Option<AsyncFunctionEnvMut<T>> {
        // Validate exact attachment before exposing a deferred environment handle.
        let _pin = self.func_env.pin_for_store(&self.store_mut)?;
        let store = if crate::backend::sys::async_runtime::native_coroutine_active() {
            let id = self.store_mut.as_store_ref().objects().id();
            crate::backend::sys::async_runtime::current_store_loan(id)?
        } else {
            self.store_mut.as_store_async()?.store()
        };
        Some(AsyncFunctionEnvMut {
            store: AsyncFunctionEnvMutStore::Async(store),
            func_env: self.func_env.clone(),
        })
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
    // A removed/replaced environment's final T destructor must run while the
    // Store gate and stack pin still exclude parent resumption.
    split_lease: Option<VMFunctionEnvironmentLease>,
    pub(crate) func_env: FunctionEnv<T>,
    write_lock: StoreAsyncWriteLock,
}

#[cfg(feature = "experimental-async")]
impl<T> Debug for AsyncFunctionEnvMut<T>
where
    T: Send + Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.store {
            AsyncFunctionEnvMutStore::Sync(ptr) => self.func_env.as_ref(&ptr.as_ref()).fmt(f),
            AsyncFunctionEnvMutStore::Async(store) => {
                match StoreAsyncReadLock::try_acquire(store) {
                    Some(read_lock) => self.func_env.as_ref(&read_lock).fmt(f),
                    None => write!(f, "AsyncFunctionEnvMut {{ <STORE LOCKED> }}"),
                }
            }
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
            AsyncFunctionEnvMutStore::Async(store) => store.store(),
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
            Self::Async(store) => Self::Async(store.store()),
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
            let view = self.write_lock.as_store_mut();
            let key = view.inner as *const StoreInner as usize;
            let owner = self
                .func_env
                .identity
                .lookup(
                    &self.func_env.handle,
                    view.as_store_ref().objects().as_sys(),
                )
                .expect("function environment is detached or replaced");
            // SAFETY: exclusive actual Store borrow and attached exact cell.
            self.split_lease = Some(unsafe { owner.lend_for_store(key) });
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
        let key = self.write_lock.as_store_ref().inner as *const StoreInner as usize;
        let pin = match &self.split_lease {
            // SAFETY: the exclusive actual Store lock and original unique lease
            // remain held, including after the old owner slot was removed.
            Some(lease) => unsafe { lease.pin_for_store(key) },
            None => self.func_env.pin_for_store(&self.write_lock),
        }
        .expect("function environment is detached or on another Store/thread");
        FunctionEnvMut {
            frame: EnvironmentFrame::Owned(pin),
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
    fn an_import_frame_retains_its_original_cell_and_rejects_replaced_callbacks() {
        for typed in [true, false] {
            let mut store = Store::default();
            let drops = Arc::new(AtomicUsize::new(0));
            let replaced_drops = Arc::new(AtomicUsize::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let env = crate::FunctionEnv::new(
                &mut store,
                State {
                    value: 7,
                    drops: drops.clone(),
                },
            );
            let saved = env.clone();
            let body_calls = calls.clone();
            let body_drops = drops.clone();
            let replacement = replaced_drops.clone();
            let body = move |mut current: crate::FunctionEnvMut<State>| -> i32 {
                body_calls.fetch_add(1, Ordering::SeqCst);
                let pin = current.pin_frame().unwrap();
                {
                    let mut actual = current.as_store_mut();
                    // Same-cell callback reentry gets a fresh borrow from the
                    // deepest actual Store, without borrowing the old T.
                    let mut nested = pin.try_as_env_mut(&mut actual).unwrap();
                    nested.data_mut().value += 1;
                }
                {
                    let mut actual = current.as_store_mut();
                    let mut removed =
                        VMFunctionEnvironment::list_mut(actual.objects_mut().as_sys_mut())
                            .pop()
                            .unwrap();
                    VMFunctionEnvironment::list_mut(actual.objects_mut().as_sys_mut()).push(
                        VMFunctionEnvironment::new(State {
                            value: 99,
                            drops: replacement.clone(),
                        }),
                    );
                    // The raw owner is Send. Even after leaving Store, none of
                    // its safe data APIs may race the original live frame.
                    let rejected = std::thread::spawn(move || {
                        let read = catch_unwind(AssertUnwindSafe(|| {
                            let _ = removed.as_ref();
                        }))
                        .is_err();
                        let write = catch_unwind(AssertUnwindSafe(|| {
                            let _ = removed.as_mut();
                        }))
                        .is_err();
                        let lend = catch_unwind(AssertUnwindSafe(|| removed.lend())).is_err();
                        let take =
                            catch_unwind(AssertUnwindSafe(|| removed.into_contents())).is_err();
                        read && write && lend && take
                    })
                    .join()
                    .unwrap();
                    assert!(rejected);
                    assert!(pin.try_as_env_mut(&mut actual).is_none());
                    assert_eq!(body_drops.load(Ordering::SeqCst), 0);
                }
                // Cleanup uses the same held cell despite replacement at its
                // old index, and derived temporary handles borrow that pin.
                current.as_mut().data_mut().value += 34;
                assert_eq!(current.data().value, 42);
                42
            };
            let function = if typed {
                crate::Function::new_typed_with_env(&mut store, &env, body)
            } else {
                crate::Function::new_with_env(
                    &mut store,
                    &env,
                    crate::FunctionType::new([], [crate::Type::I32]),
                    move |current, _| Ok(vec![crate::Value::I32(body(current))]),
                )
            };
            assert_eq!(
                &*function.call(&mut store, &[]).unwrap(),
                &[crate::Value::I32(42)]
            );
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(catch_unwind(AssertUnwindSafe(|| function.call(&mut store, &[]))).is_err());
            assert!(catch_unwind(AssertUnwindSafe(|| saved.as_ref(&store))).is_err());
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "stale callback must not enter its body"
            );
            drop(store);
            assert_eq!(replaced_drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn a_frame_pin_rejects_split_callbacks_and_spoofed_store_transplants() {
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let env = FunctionEnv::new(
            &mut store,
            State {
                value: 7,
                drops: drops.clone(),
            },
        );
        let saved = env.clone();
        let mut call = env.into_mut(&mut store);
        let pin = call.pin_frame();
        {
            let (data, mut actual) = call.data_and_store_mut();
            assert!(pin.try_as_env_mut(&mut actual).is_none());
            data.value += 1;
        }
        // Finish the split without releasing the frame's exact allocation.
        drop(call.split_lease.take());
        let original_id = call.store_mut.as_store_ref().objects().id();
        let removed = VMFunctionEnvironment::list_mut(call.objects_mut().as_sys_mut())
            .pop()
            .unwrap();
        // Isolate allocation identity from thread identity: even on this
        // thread, copying the numeric StoreId cannot retarget a live frame.
        let mut other = Store::default();
        other.objects_mut().as_sys_mut().set_id(original_id);
        VMFunctionEnvironment::list_mut(other.objects_mut().as_sys_mut()).push(removed);
        let same_thread_read = catch_unwind(AssertUnwindSafe(|| {
            let _ = saved.as_ref(&other);
        }))
        .is_err();
        let same_thread_write = catch_unwind(AssertUnwindSafe(|| {
            let _ = saved.as_mut(&mut other);
        }))
        .is_err();
        let same_thread_frame = catch_unwind(AssertUnwindSafe(|| {
            let _ = saved.clone().into_mut(&mut other);
        }))
        .is_err();
        assert!(pin.try_as_env_mut(&mut other).is_none());
        let removed = VMFunctionEnvironment::list_mut(other.objects_mut().as_sys_mut())
            .pop()
            .unwrap();
        drop(other);
        assert!(same_thread_read && same_thread_write && same_thread_frame);
        let moved_handle = saved.clone();
        let worker = std::thread::spawn(move || {
            let mut other = Store::default();
            other.objects_mut().as_sys_mut().set_id(original_id);
            VMFunctionEnvironment::list_mut(other.objects_mut().as_sys_mut()).push(removed);
            let read_rejected = catch_unwind(AssertUnwindSafe(|| {
                let _ = moved_handle.as_ref(&other);
            }))
            .is_err();
            let write_rejected = catch_unwind(AssertUnwindSafe(|| {
                let _ = moved_handle.as_mut(&mut other);
            }))
            .is_err();
            let frame_rejected = catch_unwind(AssertUnwindSafe(|| {
                let _ = moved_handle.into_mut(&mut other);
            }))
            .is_err();
            let owner = VMFunctionEnvironment::list_mut(other.objects_mut().as_sys_mut())
                .pop()
                .unwrap();
            (owner, read_rejected && write_rejected && frame_rejected)
        });
        let (owner, rejected) = worker.join().unwrap();
        assert!(rejected);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        // Rejections must neither poison metadata nor redirect original cleanup.
        call.data_mut().value += 34;
        assert_eq!(call.data().value, 42);
        let other_id = Store::default().as_store_ref().objects().id();
        call.objects_mut().as_sys_mut().set_id(other_id);
        assert_eq!(call.as_mut().data().value, 42);
        drop(owner);
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(pin);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_escaped_frame_pin_owns_no_store_or_safe_data_access() {
        let mut store = Store::default();
        let drops = Arc::new(AtomicUsize::new(0));
        let env = FunctionEnv::new(
            &mut store,
            State {
                value: 42,
                drops: drops.clone(),
            },
        );
        let call = env.into_mut(&mut store);
        let pin = call.pin_frame();
        drop(call);
        drop(store);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let mut other = Store::default();
        assert!(pin.try_as_env_mut(&mut other).is_none());
        drop(pin);
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
