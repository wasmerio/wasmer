use std::ops::{Deref, DerefMut};

use super::{inner::StoreInner, StoreObjects};
use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
use wasmer_types::{BoxStoreObject, ExternType, OnCalledAction};
//use wasmer_vm::{StoreObjects, TrapHandlerFn};

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// A temporary handle to a [`crate::Store`].
pub struct StoreRef<'a, Object = BoxStoreObject> {
    pub(crate) inner: &'a StoreInner<Object>,
}

impl<Object> std::fmt::Debug for StoreRef<'_, Object> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("StoreRef").field("inner", &self.inner).finish()
    }
}

impl<'a, Object> StoreRef<'a, Object> {
    pub(crate) fn objects(&self) -> &'a StoreObjects<Object> {
        &self.inner.objects
    }

    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        StoreObjects::<Object>::same(&a.inner.objects, &b.inner.objects)
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
pub struct StoreMut<'a, Object = BoxStoreObject> {
    pub(crate) inner: &'a mut StoreInner<Object>,
}

impl<Object> StoreMut<'_, Object> {
    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        StoreObjects::<Object>::same(&a.inner.objects, &b.inner.objects)
    }

    #[allow(unused)]
    pub(crate) fn as_raw(&self) -> *mut StoreInner<Object> {
        self.inner as *const StoreInner<Object> as *mut StoreInner<Object>
    }

    #[allow(unused)]
    pub(crate) unsafe fn from_raw(raw: *mut StoreInner<Object>) -> Self {
        Self { inner: &mut *raw }
    }

    #[allow(unused)]
    pub(crate) fn engine_and_objects_mut(&mut self) -> (&Engine, &mut StoreObjects<Object>) {
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
    type Object;

    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_, Self::Object>;
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> StoreMut<'_, Self::Object>;

    /// Returns the ObjectMutable
    fn objects_mut(&mut self) -> &mut StoreObjects<Self::Object>;
}

impl<Object> AsStoreRef for StoreRef<'_, Object> {
    type Object = Object;

    fn as_store_ref(&self) -> StoreRef<'_, Object> {
        StoreRef { inner: self.inner }
    }
}

impl<Object> AsEngineRef for StoreRef<'_, Object> {
    type Object = ();

    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}

impl<Object> AsStoreRef for StoreMut<'_, Object> {
    type Object = Object;

    fn as_store_ref(&self) -> StoreRef<'_, Object> {
        StoreRef { inner: self.inner }
    }
}
impl<Object> AsStoreMut for StoreMut<'_, Object> {
    fn as_store_mut(&mut self) -> StoreMut<'_, Object> {
        StoreMut { inner: self.inner }
    }

    fn objects_mut(&mut self) -> &mut StoreObjects<Object> {
        &mut self.inner.objects
    }
}

impl<P> AsStoreRef for P
where
    P: Deref,
    P::Target: AsStoreRef,
{
    type Object = <P::Target as AsStoreRef>::Object;

    fn as_store_ref(&self) -> StoreRef<'_, Self::Object> {
        (**self).as_store_ref()
    }
}

impl<P> AsStoreMut for P
where
    P: DerefMut,
    P::Target: AsStoreMut,
{
    fn as_store_mut(&mut self) -> StoreMut<'_, Self::Object> {
        (**self).as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects<Self::Object> {
        (**self).objects_mut()
    }
}

impl<Object> AsEngineRef for StoreMut<'_, Object> {
    type Object = std::convert::Infallible;

    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }
}
