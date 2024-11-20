use crate::{
    entities::{
        engine::{AsEngineRef, Engine},
        store::{StoreMut, StoreObjects},
    },
    AsStoreMut,
};

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    pub(crate) store: RuntimeStore,
    pub(crate) on_called: Option<OnCalledHandler>,
}

impl std::fmt::Debug for StoreInner {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("StoreInner")
            .field("objects", &self.objects)
            .field("store", &self.store)
            .field("on_called", &"<...>")
            .finish()
    }
}

/// Call handler for a store.
// TODO: better documentation!
pub type OnCalledHandler = Box<
    dyn FnOnce(
        StoreMut<'_>,
    )
        -> Result<wasmer_types::OnCalledAction, Box<dyn std::error::Error + Send + Sync>>,
>;

#[derive(derive_more::From, derive_more::Debug)]
pub(crate) enum RuntimeStore {
    #[cfg(feature = "sys")]
    Sys(crate::rt::sys::entities::store::Store),
    #[cfg(feature = "wamr")]
    Wamr(crate::rt::wamr::entities::store::Store),
    #[cfg(feature = "v8")]
    V8(crate::rt::v8::entities::store::Store),
    #[cfg(feature = "js")]
    Js(crate::rt::js::entities::store::Store),
}

impl RuntimeStore {
    pub(crate) fn engine(&self) -> &Engine {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.engine(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.engine(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.engine(),
            #[cfg(feature = "js")]
            Self::Js(s) => s.engine(),
        }
    }

    pub(crate) fn engine_mut(&mut self) -> &mut Engine {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.engine_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.engine_mut(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.engine_mut(),
            #[cfg(feature = "js")]
            Self::Js(s) => s.engine_mut(),
        }
    }
}

impl AsEngineRef for RuntimeStore {
    fn as_engine_ref(&self) -> crate::EngineRef<'_> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.as_engine_ref(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.as_engine_ref(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.as_engine_ref(),
            #[cfg(feature = "js")]
            Self::Js(s) => s.as_engine_ref(),
        }
    }
}
