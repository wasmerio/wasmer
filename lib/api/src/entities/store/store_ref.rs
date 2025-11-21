use std::ops::{Deref, DerefMut};

use super::{local_rwlock::{LocalReadGuardRc, LocalWriteGuardRc}, inner::StoreInner, StoreObjects};
use crate::{
    Store,
    entities::engine::{AsEngineRef, Engine, EngineRef},
};
use wasmer_types::{ExternType, OnCalledAction, StoreId};

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// A temporary handle to a [`crate::Store`].
pub struct StoreRef {
    pub(crate) inner: LocalReadGuardRc<StoreInner>,
}

impl StoreRef {
    fn as_ref(&self) -> &StoreInner {
        &self.inner
    }
}

/// A temporary handle to a [`crate::Store`].
pub struct StoreMut {
    pub(crate) inner: LocalWriteGuardRc<StoreInner>,

    // Also keep an Arc to the store itself, so we can recreate
    // the store for async functions.
    pub(crate) store_handle: Store,
}

impl StoreMut {
    pub(crate) fn as_ref(&self) -> &StoreInner {
        &self.inner
    }

    pub(crate) fn as_mut(&mut self) -> &mut StoreInner {
        &mut self.inner
    }

    // TODO: OnCalledAction is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
    /// Sets the unwind callback which will be invoked when the call finishes
    fn on_called<F>(&mut self, callback: F)
    where
        F: FnOnce(
                &mut StoreMut,
            ) -> Result<OnCalledAction, Box<dyn std::error::Error + Send + Sync>>
            + Send
            + Sync
            + 'static,
    {
        self.as_mut().on_called.replace(Box::new(callback));
    }
}

/// Helper trait for a value that provides immutable access to a [`Store`](crate::entities::Store).
pub trait AsStoreRef {
    /// Returns a reference to the inner store.
    fn as_ref(&self) -> &StoreInner;

    /// Returns a reference to the store objects.
    fn objects(&self) -> &StoreObjects {
        &self.as_ref().objects
    }

    /// Returns the [`Engine`].
    fn engine(&self) -> &Engine {
        self.as_ref().store.engine()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    fn same(&self, other: &dyn AsStoreRef) -> bool {
        StoreObjects::same(&self.as_ref().objects, &other.as_ref().objects)
    }

    /// The signal handler
    #[cfg(feature = "sys")]
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        use crate::backend::sys::entities::store::NativeStoreExt;
        self.as_ref().store.as_sys().signal_handler()
    }
}

/// Helper trait for a value that provides mutable access to a [`Store`](crate::entities::Store).
pub trait AsStoreMut: AsStoreRef {
    /// Returns a mutable reference to the inner store.
    fn as_mut(&mut self) -> &mut StoreInner;

    /// Re-borrow this as a mutable reference to the underlying StoreMut.
    /// This is useful for passing a generic `&mut impl AsStoreMut` to
    /// non-generic functions.
    fn reborrow_mut(&mut self) -> &mut StoreMut;

    /// Attempts to take the [`StoreMut`] instance out of this implementor.
    fn take(&mut self) -> Option<StoreMut> {
        None
    }

    /// Place the [`StoreMut`] instance back in this implementor.
    fn put_back(&mut self, store_mut: StoreMut) {
        panic!("Not supported")
    }

    /// Returns a mutable reference to the store objects.
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.as_mut().objects
    }

    /// Returns the [`Engine`].
    fn engine_mut(&mut self) -> &mut Engine {
        self.as_mut().store.engine_mut()
    }

    /// Returns mutable references to the engine and store objects.
    fn engine_and_objects_mut(&mut self) -> (&Engine, &mut StoreObjects) {
        let self_ref = self.as_mut();
        (self_ref.store.engine(), &mut self_ref.objects)
    }
}

impl AsStoreRef for StoreRef {
    fn as_ref(&self) -> &StoreInner {
        self.as_ref()
    }
}

impl AsStoreRef for StoreMut {
    fn as_ref(&self) -> &StoreInner {
        self.as_ref()
    }
}

impl AsStoreMut for StoreMut {
    fn as_mut(&mut self) -> &mut StoreInner {
        self.as_mut()
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        self
    }
}

impl<P> AsStoreRef for P
where
    P: Deref,
    P::Target: AsStoreRef,
{
    fn as_ref(&self) -> &StoreInner {
        (**self).as_ref()
    }
}

impl<P> AsStoreMut for P
where
    P: DerefMut,
    P::Target: AsStoreMut,
{
    fn as_mut(&mut self) -> &mut StoreInner {
        (**self).as_mut()
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        (**self).reborrow_mut()
    }

    fn take(&mut self) -> Option<StoreMut> {
        (**self).take()
    }

    fn put_back(&mut self, store_mut: StoreMut) {
        (**self).put_back(store_mut)
    }
}

impl AsEngineRef for StoreRef {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.as_ref().store.as_engine_ref()
    }
}

impl AsEngineRef for StoreMut {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.as_ref().store.as_engine_ref()
    }
}
