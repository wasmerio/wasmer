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

    // Host functions retain raw pointers back to this allocation. Making the
    // pointee !Unpin suppresses rustc's `&mut StoreInner` noalias annotations:
    // guest calls can re-enter the store through those pointers.
    pub(crate) _marker: std::marker::PhantomPinned,
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
