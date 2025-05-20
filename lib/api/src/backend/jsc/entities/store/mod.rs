pub(crate) mod handle;
pub(crate) mod obj;

pub(crate) use handle::*;
pub(crate) use obj::*;

use crate::{AsEngineRef, Engine, EngineRef};

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
    /// Consume [`self`] into [`crate::backend::jsc::store::Store`].
    pub fn into_jsc(self) -> crate::backend::jsc::store::Store {
        match self {
            Self::Jsc(s) => s,
            _ => panic!("Not a `jsc` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::store::Store`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::store::Store {
        match self {
            Self::Jsc(s) => s,
            _ => panic!("Not a `jsc` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::store::Store`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::store::Store {
        match self {
            Self::Jsc(s) => s,
            _ => panic!("Not a `jsc` store!"),
        }
    }
    /// Return true if [`self`] is a store from the `jsc` runtime.
    pub fn is_jsc(&self) -> bool {
        matches!(self, Self::Jsc(_))
    }
}

impl crate::Store {
    /// Consume [`self`] into [`crate::backend::jsc::store::Store`].
    pub(crate) fn into_jsc(self) -> crate::backend::jsc::store::Store {
        self.inner.store.into_jsc()
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::store::Store`].
    pub(crate) fn as_jsc(&self) -> &crate::backend::jsc::store::Store {
        self.inner.store.as_jsc()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::store::Store`].
    pub(crate) fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::store::Store {
        self.inner.store.as_jsc_mut()
    }

    /// Return true if [`self`] is a store from the `jsc` runtime.
    pub fn is_jsc(&self) -> bool {
        self.inner.store.is_jsc()
    }
}
