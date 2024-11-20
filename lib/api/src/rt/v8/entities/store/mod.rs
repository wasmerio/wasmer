//! Data types, functions and traits for `v8` runtime's `Store` implementation.
use crate::{
    engine::{AsEngineRef, Engine, EngineRef},
    rt::v8::bindings::{wasm_store_delete, wasm_store_new, wasm_store_t},
    AsStoreRef, RuntimeStore, StoreRef,
};

mod obj;
pub use obj::*;

/// A WebAssembly `store` in the `v8` runtime.
pub(crate) struct Store {
    pub(crate) engine: Engine,
    pub(crate) inner: *mut wasm_store_t,
}

impl Store {
    pub(crate) fn new(engine: crate::engine::Engine) -> Self {
        let inner: *mut wasm_store_t = unsafe { wasm_store_new(engine.as_v8().inner.engine) };
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
    /// Consume [`self`] into [`crate::rt::v8::store::Store`].
    pub fn into_v8(self) -> crate::rt::v8::store::Store {
        match self {
            Self::V8(s) => s,
            _ => panic!("Not a `v8` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::v8::store::Store`].
    pub fn as_v8(&self) -> &crate::rt::v8::store::Store {
        match self {
            Self::V8(s) => s,
            _ => panic!("Not a `v8` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::v8::store::Store`].
    pub fn as_v8_mut(&mut self) -> &mut crate::rt::v8::store::Store {
        match self {
            Self::V8(s) => s,
            _ => panic!("Not a `v8` store!"),
        }
    }

    /// Return true if [`self`] is a store from the `v8` runtime.
    pub fn is_v8(&self) -> bool {
        matches!(self, Self::V8(_))
    }
}

impl crate::Store {
    /// Consume [`self`] into [`crate::rt::v8::store::Store`].
    pub(crate) fn into_v8(self) -> crate::rt::v8::store::Store {
        self.inner.store.into_v8()
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::v8::store::Store`].
    pub(crate) fn as_v8(&self) -> &crate::rt::v8::store::Store {
        self.inner.store.as_v8()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::v8::store::Store`].
    pub(crate) fn as_v8_mut(&mut self) -> &mut crate::rt::v8::store::Store {
        self.inner.store.as_v8_mut()
    }

    /// Return true if [`self`] is a store from the `v8` runtime.
    pub fn is_v8(&self) -> bool {
        self.inner.store.is_v8()
    }
}
