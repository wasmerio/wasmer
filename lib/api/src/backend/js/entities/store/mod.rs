use crate::{AsEngineRef, Engine, EngineRef};

pub(crate) mod handle;
pub(crate) mod obj;

pub(crate) use handle::*;
pub(crate) use obj::*;

#[derive(Debug)]
pub(crate) struct Store {
    pub(crate) engine: Engine,
}

impl Store {
    pub(crate) fn new(engine: Engine) -> Self {
        Self { engine }
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub(crate) fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
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
            Self::Js(s) => s,
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
    /// Consume [`self`] into [`crate::backend::js::store::Store`].
    pub(crate) fn into_js(self) -> crate::backend::js::store::Store {
        self.inner.store.into_js()
    }

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
