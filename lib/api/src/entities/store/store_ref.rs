use std::ops::{Deref, DerefMut};

use super::{StoreObjects, inner::StoreInner};
use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync};
use wasmer_types::{ExternType, OnCalledAction};
//use wasmer_vm::{StoreObjects, TrapHandlerFn};

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// A temporary handle to a [`crate::Store`].
#[derive(Debug)]
pub struct StoreRef<'a> {
    pub(crate) inner: &'a StoreInner,
}

impl<'a> StoreRef<'a> {
    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }

    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        StoreObjects::same(&a.inner.objects, &b.inner.objects)
    }

    /// The signal handler
    #[cfg(feature = "sys")]
    #[inline]
    pub fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        use crate::backend::sys::entities::store::NativeStoreExt;
        self.inner.store.as_sys().signal_handler()
    }
}

/// A temporary handle to a [`crate::Store`].
pub struct StoreMut<'a> {
    pub(crate) inner: &'a mut StoreInner,
}

impl StoreMut<'_> {
    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        StoreObjects::same(&a.inner.objects, &b.inner.objects)
    }

    #[allow(unused)]
    pub(crate) fn as_raw(&self) -> *mut StoreInner {
        self.inner as *const StoreInner as *mut StoreInner
    }

    #[allow(unused)]
    pub(crate) unsafe fn from_raw(raw: *mut StoreInner) -> Self {
        Self {
            inner: unsafe { &mut *raw },
        }
    }

    #[allow(unused)]
    pub(crate) fn engine_and_objects_mut(&mut self) -> (&Engine, &mut StoreObjects) {
        (self.inner.store.engine(), &mut self.inner.objects)
    }

    /// Parks this borrow of the store for the duration of `f`, lending the
    /// store to code that runs inside `f` without reaching it through Rust:
    /// [`Store::with_current`](crate::Store::with_current) hands the store back to any frame `f` reaches,
    /// however many foreign frames deep.
    ///
    /// This is how host code lends the store across an FFI boundary. An
    /// embedded engine that calls back into the host — a JS engine's allocator
    /// hook, say — cannot be handed a Rust reference through its own C frames,
    /// and cannot be given one out of band either, because the imported
    /// function it was called from is still holding the only borrow. Parking
    /// that borrow, which the `&mut self` receiver here makes unreachable for
    /// exactly as long as `f` runs, is what makes lending it sound rather than
    /// aliasing.
    ///
    /// Parks nest, and a store nobody parked stays unlendable:
    /// [`Store::with_current`](crate::Store::with_current) returns `None`
    /// unless every borrow on this thread's stack has been parked this way.
    ///
    /// ```
    /// use wasmer::{AsStoreMut, FunctionEnvMut, Store};
    ///
    /// // A host function lending its store to code it calls into.
    /// fn host_call(mut env: FunctionEnvMut<'_, ()>) {
    ///     // `env` holds the store, so nobody else may have it.
    ///     assert!(Store::with_current(|_| ()).is_none());
    ///
    ///     env.as_store_mut().parked(|| {
    ///         // ...but code reached from here can pick it back up.
    ///         assert!(Store::with_current(|_| ()).is_some());
    ///     });
    /// }
    /// ```
    pub fn parked<R>(&mut self, f: impl FnOnce() -> R) -> R {
        // This is the same thing `Function::call` does before entering Wasm:
        // install this borrow as the store executing on the thread, so that
        // code which reaches the store through the context — rather than
        // through a reference it was handed — picks up a borrow derived from
        // *this* one. The new entry starts unborrowed, which is what makes the
        // store lendable for as long as it is on top.
        let ptr: *mut StoreInner = &mut *self.inner;
        // SAFETY: `ptr` comes from `&mut *self.inner`, which `&mut self` keeps
        // alive and unreachable for every frame on this thread until `f`
        // returns and the guard is dropped.
        let _guard = unsafe { super::StoreContext::install(ptr) };
        f()
    }

    // TODO: OnCalledAction is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
    /// Sets the unwind callback which will be invoked when the call finishes
    pub fn on_called<F>(&mut self, callback: F)
    where
        F: FnOnce(StoreMut<'_>) -> Result<OnCalledAction, Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        self.inner.on_called.replace(Box::new(callback));
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_>;

    /// Returns a [`StoreAsync`] if the current
    /// context is asynchronous. The store will be locked since
    /// it's already active in the current context, but can be used
    /// to spawn new coroutines via
    /// [`Function::call_async`](crate::Function::call_async).
    #[cfg(feature = "experimental-async")]
    fn as_store_async(&self) -> Option<impl AsStoreAsync + 'static> {
        let id = self.as_store_ref().inner.objects.id();
        StoreAsync::from_context(id)
    }
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> StoreMut<'_>;

    /// Returns the ObjectMutable
    fn objects_mut(&mut self) -> &mut StoreObjects;
}

impl AsStoreRef for StoreRef<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}

impl AsEngineRef for StoreRef<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}

impl AsStoreRef for StoreMut<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}
impl AsStoreMut for StoreMut<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut { inner: self.inner }
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}

impl<P> AsStoreRef for P
where
    P: Deref,
    P::Target: AsStoreRef,
{
    fn as_store_ref(&self) -> StoreRef<'_> {
        (**self).as_store_ref()
    }
}

impl<P> AsStoreMut for P
where
    P: DerefMut,
    P::Target: AsStoreMut,
{
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        (**self).as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        (**self).objects_mut()
    }
}

impl AsEngineRef for StoreMut<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}
