//! Data types, functions and traits for `sys` runtime's `Store` implementation.
use crate::entities::engine::{AsEngineRef, Engine, EngineRef};
use crate::BackendStore;
use wasmer_vm::init_traps;
use wasmer_vm::TrapHandlerFn;
pub use wasmer_vm::{StoreHandle, StoreObjects};

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

impl NativeStoreExt for crate::Store {
    fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        self.inner.store.as_sys_mut().set_trap_handler(handler)
    }

    /// The signal handler
    #[inline]
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        self.inner.store.as_sys().signal_handler()
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

impl crate::Store {
    /// Consume [`self`] into [`crate::backend::sys::store::Store`].
    pub(crate) fn into_sys(self) -> crate::backend::sys::store::Store {
        self.inner.store.into_sys()
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::sys::store::Store`].
    pub(crate) fn as_sys(&self) -> &crate::backend::sys::store::Store {
        self.inner.store.as_sys()
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::sys::store::Store`].
    pub(crate) fn as_sys_mut(&mut self) -> &mut crate::backend::sys::store::Store {
        self.inner.store.as_sys_mut()
    }

    /// Return true if [`self`] is a store from the `sys` runtime.
    pub fn is_sys(&self) -> bool {
        self.inner.store.is_sys()
    }
}
