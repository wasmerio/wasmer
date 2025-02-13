use std::ops::{Deref, DerefMut};

use super::{inner::StoreInner, StoreObjects};
use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
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

impl<'a> StoreMut<'a> {
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
        Self { inner: &mut *raw }
    }

    #[allow(unused)]
    pub(crate) fn engine_and_objects_mut(&mut self) -> (&Engine, &mut StoreObjects) {
        (self.inner.store.engine(), &mut self.inner.objects)
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
