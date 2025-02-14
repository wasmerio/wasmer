//! Defines the [`Store`] data type and various useful traits and data types to interact with a
//! store.

/// Defines the [`StoreInner`] data type.
mod inner;

/// Create temporary handles to engines.
mod store_ref;
pub use store_ref::*;

mod obj;
pub use obj::*;

use crate::{AsEngineRef, BackendEngine, Engine, EngineRef};
pub(crate) use inner::*;
use wasmer_types::StoreId;

#[cfg(feature = "sys")]
use wasmer_vm::TrapHandlerFn;

/// The store represents all global state that can be manipulated by
/// WebAssembly programs. It consists of the runtime representation
/// of all instances of functions, tables, memories, and globals that
/// have been allocated during the lifetime of the abstract machine.
///
/// The [`Store`] is tied to the underlying [`Engine`] that is — among many things — used to
/// compile the Wasm bytes into a valid module artifact.
///
/// For more informations, check out the [related WebAssembly specification]
/// [related WebAssembly specification]: <https://webassembly.github.io/spec/core/exec/runtime.html#store>
pub struct Store {
    pub(crate) inner: Box<StoreInner>,
}

impl Store {
    /// Creates a new `Store` with a specific [`Engine`].
    pub fn new(engine: impl Into<Engine>) -> Self {
        let engine: Engine = engine.into();

        let store = match engine.be {
            #[cfg(feature = "sys")]
            BackendEngine::Sys(_) => {
                BackendStore::Sys(crate::backend::sys::entities::store::Store::new(engine))
            }
            #[cfg(feature = "wamr")]
            BackendEngine::Wamr(_) => {
                BackendStore::Wamr(crate::backend::wamr::entities::store::Store::new(engine))
            }
            #[cfg(feature = "wasmi")]
            BackendEngine::Wasmi(_) => {
                BackendStore::Wasmi(crate::backend::wasmi::entities::store::Store::new(engine))
            }
            #[cfg(feature = "v8")]
            BackendEngine::V8(_) => {
                BackendStore::V8(crate::backend::v8::entities::store::Store::new(engine))
            }
            #[cfg(feature = "js")]
            BackendEngine::Js(_) => {
                BackendStore::Js(crate::backend::js::entities::store::Store::new(engine))
            }
            #[cfg(feature = "jsc")]
            BackendEngine::Jsc(_) => {
                BackendStore::Jsc(crate::backend::jsc::entities::store::Store::new(engine))
            }
        };

        Self {
            inner: Box::new(StoreInner {
                objects: StoreObjects::from_store_ref(&store),
                on_called: None,
                store,
            }),
        }
    }

    #[cfg(feature = "sys")]
    /// Set the [`TrapHandlerFn`] for this store.
    ///
    /// # Note
    ///
    /// Not every implementor allows changing the trap handler. In those store that
    /// don't allow it, this function has no effect.
    pub fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        use crate::backend::sys::entities::store::NativeStoreExt;
        #[allow(irrefutable_let_patterns)]
        if let BackendStore::Sys(ref mut s) = self.inner.store {
            s.set_trap_handler(handler)
        }
    }

    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        self.inner.store.engine()
    }

    /// Returns mutable reference to [`Engine`].
    pub fn engine_mut(&mut self) -> &mut Engine {
        self.inner.store.engine_mut()
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.id() == b.id()
    }

    /// Returns the ID of this store
    pub fn id(&self) -> StoreId {
        self.inner.objects.id()
    }
}

impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Self::same(self, other)
    }
}

// This is required to be able to set the trap_handler in the
// Store.
unsafe impl Send for Store {}
unsafe impl Sync for Store {}

impl Default for Store {
    fn default() -> Self {
        Self::new(Engine::default())
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Store").finish()
    }
}

impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        self.inner.store.as_engine_ref()
    }

    fn maybe_as_store(&self) -> Option<StoreRef<'_>> {
        Some(self.as_store_ref())
    }
}

impl AsStoreRef for Store {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: &self.inner }
    }
}
impl AsStoreMut for Store {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut {
            inner: &mut self.inner,
        }
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}
