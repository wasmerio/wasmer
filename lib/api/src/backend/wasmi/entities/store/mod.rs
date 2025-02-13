//! Data types, functions and traits for `wasmi`'s `Store` implementation.
use crate::{
    backend::wasmi::bindings::{wasm_store_delete, wasm_store_new, wasm_store_t},
    engine::{AsEngineRef, Engine, EngineRef},
    AsStoreRef, BackendStore, StoreRef,
};

mod obj;
pub use obj::*;

/// A WebAssembly `store` in `wasmi`.
pub(crate) struct Store {
    pub(crate) engine: Engine,
    pub(crate) inner: *mut wasm_store_t,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("engine", &self.engine)
            .finish()
    }
}

impl Store {
    pub(crate) fn new(engine: crate::engine::Engine) -> Self {
        let inner: *mut wasm_store_t = unsafe { wasm_store_new(engine.as_wasmi().inner.engine) };
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

impl crate::BackendStore {
    /// Consume [`self`] into [`crate::backend::wasmi::store::Store`].
    pub fn into_wasmi(self) -> crate::backend::wasmi::store::Store {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::wasmi::store::Store`].
    pub fn as_wasmi(&self) -> &crate::backend::wasmi::store::Store {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::wasmi::store::Store`].
    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::store::Store {
        match self {
            Self::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` store!"),
        }
    }

    /// Return true if [`self`] is a store from the `wasmi` runtime.
    pub fn is_wasmi(&self) -> bool {
        matches!(self, Self::Wasmi(_))
    }
}

impl crate::Store {
    /// Consume [`self`] into [`crate::backend::wasmi::store::Store`].
    pub(crate) fn into_wasmi(self) -> crate::backend::wasmi::store::Store {
        self.inner.store.into_wasmi()
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::wasmi::store::Store`].
    pub(crate) fn as_wasmi(&self) -> &crate::backend::wasmi::store::Store {
        self.inner.store.as_wasmi()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::wasmi::store::Store`].
    pub(crate) fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::store::Store {
        self.inner.store.as_wasmi_mut()
    }

    /// Return true if [`self`] is a store from the `wasmi` runtime.
    pub fn is_wasmi(&self) -> bool {
        self.inner.store.is_wasmi()
    }
}
