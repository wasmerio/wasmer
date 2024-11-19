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
#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    #[derivative(Debug = "ignore")]
    pub(crate) store: RuntimeStore,
    #[derivative(Debug = "ignore")]
    pub(crate) on_called: Option<OnCalledHandler>,
}

/// Call handler for a store.
// TODO: better documentation!
pub type OnCalledHandler = Box<
    dyn FnOnce(
        StoreMut<'_>,
    )
        -> Result<wasmer_types::OnCalledAction, Box<dyn std::error::Error + Send + Sync>>,
>;

#[derive(derive_more::From)]
pub(crate) enum RuntimeStore {
    #[cfg(feature = "sys")]
    Sys(crate::rt::sys::entities::store::Store),
    #[cfg(feature = "wamr")]
    Wamr(crate::rt::wamr::entities::store::Store),
    #[cfg(feature = "v8")]
    V8(crate::rt::v8::entities::store::Store),
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
            _ => panic!("No runtime enabled!"),
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
            _ => panic!("No runtime enabled!"),
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
            _ => panic!("No runtime enabled!"),
        }
    }
}
