use crate::{AsEngineRef, Engine, EngineRef};

pub(crate) mod handle;
pub(crate) mod obj;
pub(crate) mod slot;

pub(crate) use handle::*;
pub(crate) use obj::*;
pub(crate) use slot::*;

#[derive(Debug)]
pub(crate) struct Store {
    pub(crate) engine: Engine,
    /// Where the host functions of the store find it.
    pub(crate) slot: StoreSlot,
}

impl Store {
    pub(crate) fn new(engine: Engine) -> Self {
        Self {
            engine,
            slot: StoreSlot::new(),
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
        self.slot.clear();
    }
}

impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}

impl crate::BackendStore {
    /// Consume [`self`] into [`crate::backend::js::store::Store`].
    pub fn into_js(self) -> crate::backend::js::store::Store {
        match self {
            Self::Js(s) => {
                // Taken out of its store, which is dropped.
                s.slot.clear();
                s
            }
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::store::Store`].
    pub fn as_js(&self) -> &crate::backend::js::store::Store {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::store::Store`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::store::Store {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }
    /// Return true if [`self`] is a store from the `js` runtime.
    pub fn is_js(&self) -> bool {
        matches!(self, Self::Js(_))
    }
}

impl crate::Store {
    /// Convert a reference to [`self`] into a reference [`crate::backend::js::store::Store`].
    pub(crate) fn as_js(&self) -> &crate::backend::js::store::Store {
        self.inner.store.as_js()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::store::Store`].
    pub(crate) fn as_js_mut(&mut self) -> &mut crate::backend::js::store::Store {
        self.inner.store.as_js_mut()
    }

    /// Return true if [`self`] is a store from the `js` runtime.
    pub fn is_js(&self) -> bool {
        self.inner.store.is_js()
    }
}
