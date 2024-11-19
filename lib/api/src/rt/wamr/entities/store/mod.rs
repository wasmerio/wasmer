use crate::{
    engine::{AsEngineRef, Engine, EngineRef},
    rt::wamr::bindings::{wasm_store_delete, wasm_store_new, wasm_store_t},
    AsStoreRef, RuntimeStore, StoreRef,
};

mod obj;
pub use obj::*;

pub(crate) struct Store {
    pub(crate) engine: Engine,
    pub(crate) inner: *mut wasm_store_t,
}

impl Store {
    pub(crate) fn new(engine: crate::engine::Engine) -> Self {
        let inner: *mut wasm_store_t = unsafe { wasm_store_new(engine.as_wamr().inner.engine) };
        Store { inner, engine }
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(crate) fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        unsafe { wasm_store_delete(self.inner) }
    }
}

impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}

impl crate::RuntimeStore {
    /// Consume [`self`] into [`crate::rt::wamr::store::Store`].
    pub fn into_wamr(self) -> crate::rt::wamr::store::Store {
        match self {
            Self::Wamr(s) => s,
            _ => panic!("Not a `wamr` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::wamr::store::Store`].
    pub fn as_wamr(&self) -> &crate::rt::wamr::store::Store {
        match self {
            Self::Wamr(s) => s,
            _ => panic!("Not a `wamr` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::wamr::store::Store`].
    pub fn as_wamr_mut(&mut self) -> &mut crate::rt::wamr::store::Store {
        match self {
            Self::Wamr(s) => s,
            _ => panic!("Not a `wamr` store!"),
        }
    }

    /// Return true if [`self`] is a store from the `wamr` runtime.
    pub fn is_wamr(&self) -> bool {
        matches!(self, Self::Wamr(_))
    }
}

impl crate::Store {
    /// Consume [`self`] into [`crate::rt::wamr::store::Store`].
    pub(crate) fn into_wamr(self) -> crate::rt::wamr::store::Store {
        self.inner.store.into_wamr()
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::wamr::store::Store`].
    pub(crate) fn as_wamr(&self) -> &crate::rt::wamr::store::Store {
        self.inner.store.as_wamr()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::wamr::store::Store`].
    pub(crate) fn as_wamr_mut(&mut self) -> &mut crate::rt::wamr::store::Store {
        self.inner.store.as_wamr_mut()
    }

    /// Return true if [`self`] is a store from the `wamr` runtime.
    pub fn is_wamr(&self) -> bool {
        self.inner.store.is_wamr()
    }
}
