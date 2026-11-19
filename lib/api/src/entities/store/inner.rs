use crate::{
    AsStoreMut, AsStoreRef, StoreRef,
    entities::{
        engine::{AsEngineRef, Engine},
        store::{StoreMut, StoreObjects},
    },
    macros::backend::{gen_rt_ty, match_rt},
};

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    pub(crate) store: BackendStore,
}

impl Drop for StoreInner {
    fn drop(&mut self) {
        // Before the objects are dropped, which include the function
        // environments: the `Drop` of their values can call into a guest,
        // which can call a host function of the store. That host function
        // must not find a store that is half dropped.
        self.store.detach_host_functions();
    }
}

impl std::fmt::Debug for StoreInner {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("StoreInner")
            .field("objects", &self.objects)
            .field("store", &self.store)
            .finish()
    }
}

impl AsStoreRef for StoreInner {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self }
    }
}
impl AsStoreMut for StoreInner {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut { inner: self }
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.objects
    }
}

gen_rt_ty! {
    #[derive(derive_more::From, Debug)]
    pub BackendStore(entities::store::Store);
}

impl BackendStore {
    /// Detaches the store from its host functions, as it is dropped: a host
    /// function called from then on fails the call instead of reaching the
    /// store. Only the `js` backend's host functions can outlive their store.
    pub(crate) fn detach_host_functions(&self) {
        match self {
            #[cfg(feature = "js")]
            Self::Js(store) => store.slot.clear(),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    /// How a call into a `sys` store hands the store to its host functions:
    /// the bits of [`context_mode`](crate::context_mode), none for a plain
    /// store, whose calls go through its slot (see [`SlotRef`](super::SlotRef)).
    #[cfg(feature = "sys")]
    #[inline]
    pub(crate) fn context_mode(&self) -> u8 {
        match self {
            Self::Sys(store) => store.context_mode,
            #[allow(unreachable_patterns)]
            _ => 0,
        }
    }

    /// Sets the bits `mode` of [`Self::context_mode`], or clears them.
    #[cfg(feature = "sys")]
    pub(crate) fn set_context_mode(&mut self, mode: u8, set: bool) {
        match self {
            Self::Sys(store) if set => store.context_mode |= mode,
            Self::Sys(store) => store.context_mode &= !mode,
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    #[inline]
    pub(crate) fn engine(&self) -> &Engine {
        match_rt!(on self => s {
            s.engine()
        })
    }

    #[inline]
    pub(crate) fn engine_mut(&mut self) -> &mut Engine {
        match_rt!(on self => s {
            s.engine_mut()
        })
    }
}

impl AsEngineRef for BackendStore {
    #[inline]
    fn as_engine_ref(&self) -> crate::EngineRef<'_> {
        match_rt!(on self => s {
            s.as_engine_ref()
        })
    }
}
