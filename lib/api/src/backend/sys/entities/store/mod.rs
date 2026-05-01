//! Data types, functions and traits for `sys` runtime's `Store` implementation.
use crate::BackendStore;
use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
use wasmer_vm::TrapHandlerFn;
use wasmer_vm::init_traps;
pub use wasmer_vm::{StoreHandle, StoreId, StoreObjects};

mod obj;
pub use obj::*;

/// A WebAssembly `store` in the `sys` runtime.
pub struct Store {
    pub(crate) engine: Engine,
    pub(crate) trap_handler: Option<Box<TrapHandlerFn<'static>>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("engine", &self.engine)
            .finish()
    }
}

impl Store {
    pub(crate) fn new(engine: Engine) -> Self {
        init_traps();

        Self {
            engine,
            trap_handler: None,
        }
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

/// The custom trait to access to all the `sys` functions in the
/// Store.
pub trait NativeStoreExt {
    /// Sets the trap handler
    fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>);
    /// The signal handler
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>>;
}

impl NativeStoreExt for Store {
    fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        self.trap_handler = handler;
    }

    /// The signal handler
    #[inline]
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        self.trap_handler
            .as_ref()
            .map(|handler| handler.as_ref() as *const _)
    }
}

impl crate::BackendStore {
    /// Consume [`self`] into [`crate::backend::sys::store::Store`].
    pub fn into_sys(self) -> crate::backend::sys::store::Store {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::sys::store::Store`].
    pub fn as_sys(&self) -> &crate::backend::sys::store::Store {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::sys::store::Store`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::store::Store {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }
    /// Return true if [`self`] is a store from the `sys` runtime.
    pub fn is_sys(&self) -> bool {
        matches!(self, Self::Sys(_))
    }
}

/// Allows embedders to interrupt a running WASM instance.
#[cfg(all(unix, feature = "experimental-host-interrupt"))]
#[derive(Clone)]
pub struct Interrupter {
    store_id: StoreId,
}

#[cfg(all(unix, feature = "experimental-host-interrupt"))]
impl Interrupter {
    /// Builds a new interrupter.
    pub fn new(store_id: StoreId) -> Self {
        Self { store_id }
    }

    /// Interrupts running WASM instances from the owning `Store`.
    pub fn interrupt(&self) {
        use wasmer_vm::interrupt_registry;

        // Even though `interrupt` reports whether it sent the signal successfully,
        // there's nothing meaningful embedders can do with the result; a sent
        // signal may not be processed in rare cases, and none of the error cases
        // are hard errors in the sense that retrying the interrupt at a later
        // point is *guaranteed* to fail again. Hence, we don't return any
        // indication of success or failure to embedder code.
        match interrupt_registry::interrupt(self.store_id) {
            Err(interrupt_registry::InterruptError::StoreNotRunning) => (),
            _ => {
                #[cfg(feature = "experimental-async")]
                crate::backend::sys::async_runtime::notify_pending_futures_of_interrupt(
                    self.store_id,
                );
            }
        }
    }
}
