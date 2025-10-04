use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
use crate::BackendStore;
use wasmer_types::StoreId;

mod obj;
pub use obj::*;

/// Minimal store representation for the stub backend.
#[derive(Clone, Debug)]
pub struct Store {
    engine: Engine,
    id: StoreId,
}

impl Store {
    pub(crate) fn new(engine: Engine) -> Self {
        Self {
            engine,
            id: StoreId::default(),
        }
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(crate) fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub(crate) fn id(&self) -> StoreId {
        self.id
    }
}

impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}

impl crate::BackendStore {
    /// Consume [`self`] into a stub store.
    pub fn into_stub(self) -> crate::backend::stub::entities::store::Store {
        match self {
            BackendStore::Stub(s) => s,
            _ => panic!("Not a stub store!"),
        }
    }

    /// Borrow [`self`] as a stub store.
    pub fn as_stub(&self) -> &crate::backend::stub::entities::store::Store {
        match self {
            BackendStore::Stub(s) => s,
            _ => panic!("Not a stub store!"),
        }
    }

    /// Mutably borrow [`self`] as a stub store.
    pub fn as_stub_mut(&mut self) -> &mut crate::backend::stub::entities::store::Store {
        match self {
            BackendStore::Stub(s) => s,
            _ => panic!("Not a stub store!"),
        }
    }

    /// Return true if [`self`] refers to the stub store.
    pub fn is_stub(&self) -> bool {
        matches!(self, BackendStore::Stub(_))
    }
}

impl crate::Store {
    /// Consume [`self`] into the stub backend store.
    pub(crate) fn into_stub(self) -> crate::backend::stub::entities::store::Store {
        self.inner.store.into_stub()
    }

    /// Borrow [`self`] as a stub backend store.
    pub(crate) fn as_stub(&self) -> &crate::backend::stub::entities::store::Store {
        self.inner.store.as_stub()
    }

    /// Mutably borrow [`self`] as a stub backend store.
    pub(crate) fn as_stub_mut(&mut self) -> &mut crate::backend::stub::entities::store::Store {
        self.inner.store.as_stub_mut()
    }
}
