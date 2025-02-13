//! Data types, functions and traits for `v8` runtime's `Store` implementation.
use std::thread::ThreadId;

use crate::{
    backend::v8::bindings::{wasm_store_delete, wasm_store_new, wasm_store_t},
    engine::{AsEngineRef, Engine, EngineRef},
    AsStoreRef, BackendStore, StoreRef,
};

mod obj;
pub use obj::*;

/// A WebAssembly `store` in the `v8` runtime.
pub struct Store {
    pub(crate) engine: Engine,
    pub(crate) inner: *mut wasm_store_t,
    pub(crate) thread_id: ThreadId,
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
        let inner: *mut wasm_store_t = unsafe { wasm_store_new(engine.as_v8().inner.engine) };
        let thread_id = std::thread::current().id();
        Store {
            inner,
            engine,
            thread_id,
        }
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
    /// Consume [`self`] into [`crate::backend::v8::store::Store`].
    pub fn into_v8(self) -> crate::backend::v8::store::Store {
        match self {
            Self::V8(s) => s,
            _ => panic!("Not a `v8` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::v8::store::Store`].
    pub fn as_v8(&self) -> &crate::backend::v8::store::Store {
        match self {
            Self::V8(s) => s,
            _ => panic!("Not a `v8` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::v8::store::Store`].
    pub fn as_v8_mut(&mut self) -> &mut crate::backend::v8::store::Store {
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
    /// Consume [`self`] into [`crate::backend::v8::store::Store`].
    pub(crate) fn into_v8(self) -> crate::backend::v8::store::Store {
        self.inner.store.into_v8()
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::v8::store::Store`].
    pub(crate) fn as_v8(&self) -> &crate::backend::v8::store::Store {
        self.inner.store.as_v8()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::v8::store::Store`].
    pub(crate) fn as_v8_mut(&mut self) -> &mut crate::backend::v8::store::Store {
        self.inner.store.as_v8_mut()
    }

    /// Return true if [`self`] is a store from the `v8` runtime.
    pub fn is_v8(&self) -> bool {
        self.inner.store.is_v8()
    }
}
