use crate::engine::{AsEngineRef, Engine, EngineRef};
use wasmer_vm::init_traps;
use wasmer_vm::TrapHandlerFn;

pub(crate) struct Store {
    pub(crate) engine: Engine,

    pub(crate) trap_handler: Option<Box<TrapHandlerFn<'static>>>,
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

impl NativeStoreExt for crate::Store {
    fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        self.inner.store.set_trap_handler(handler)
    }

    /// The signal handler
    #[inline]
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        self.inner.store.signal_handler()
    }
}
