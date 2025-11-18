//! Defines the [`Store`] data type and various useful traits and data types to interact with a
//! store.

/// Defines the [`StoreContext`] type.
mod context;

/// Defines the [`StoreInner`] data type.
mod inner;

/// Create temporary handles to engines.
mod store_ref;

use async_lock::RwLock;
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, TryLockError},
};

pub use store_ref::*;

mod obj;
pub use obj::*;

use crate::{AsEngineRef, BackendEngine, Engine, EngineRef};
pub(crate) use context::*;
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
    pub(crate) inner: Arc<async_lock::RwLock<StoreInner>>,
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
            inner: std::sync::Arc::new(async_lock::RwLock::new(StoreInner {
                objects: StoreObjects::from_store_ref(&store),
                on_called: None,
                store,
            })),
        }
    }

    /// Creates a new [`StoreRef`] if the store is available for reading.
    pub(crate) fn make_ref(&self) -> Option<StoreRef> {
        self.inner
            .try_read_arc()
            .map(|guard| StoreRef { inner: guard })
    }

    /// Creates a new [`StoreRef`] if the store is available for reading.
    pub(crate) fn make_mut(&self) -> Option<StoreMut> {
        self.inner
            .try_write_arc()
            .map(|guard| StoreMut { inner: guard })
    }

    /// Builds an [`AsStoreMut`] handle to this store, provided
    /// the store is not locked. Panics if the store is already locked.
    pub fn as_mut<'a>(&'a mut self) -> impl AsStoreMut + 'a {
        StoreMutGuard {
            inner: Some(self.make_mut().expect("Store is locked")),
            marker: std::marker::PhantomData,
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
        if let BackendStore::Sys(ref mut s) = self.make_mut().expect("Store is locked").inner.store
        {
            s.set_trap_handler(handler)
        }
    }

    /// Returns the [`Engine`].
    pub fn engine<'a>(&'a self) -> StoreEngineRef<'a> {
        // Happily unwrap the read lock here because we don't expect
        // embedder code to access stores in parallel.
        StoreEngineRef {
            inner: self.make_ref().expect("Store is locked"),
            marker: std::marker::PhantomData,
        }
    }

    /// Returns mutable reference to [`Engine`].
    pub fn engine_mut<'a>(&'a mut self) -> StoreEngineMut<'a> {
        StoreEngineMut {
            inner: self.make_mut().expect("Store is locked"),
            marker: std::marker::PhantomData,
        }
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.id() == b.id()
    }

    /// Returns the ID of this store
    pub fn id(&self) -> StoreId {
        self.make_ref().expect("Store is locked").objects().id()
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

/// Marker used to make the engine accessible from a store reference.
/// Needed because the store's lock must be held while accessing the engine.
///
/// This struct borrows the [`Store`] to help prevent accidental deadlocks.
pub struct StoreEngineRef<'a> {
    inner: StoreRef,
    marker: std::marker::PhantomData<&'a ()>,
}

impl Deref for StoreEngineRef<'_> {
    type Target = Engine;

    fn deref(&self) -> &Self::Target {
        self.inner.engine()
    }
}

/// Marker used to make the engine accessible from a store reference.
/// Needed because the store's lock must be held while accessing the engine.
///
/// This struct borrows the [`Store`] to help prevent accidental deadlocks.
pub struct StoreEngineMut<'a> {
    inner: StoreMut,
    marker: std::marker::PhantomData<&'a ()>,
}

impl Deref for StoreEngineMut<'_> {
    type Target = Engine;

    fn deref(&self) -> &Self::Target {
        self.inner.engine()
    }
}

impl DerefMut for StoreEngineMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.engine_mut()
    }
}

/// A guard that provides mutable access to a [`Store`]. This is
/// the only way for embedders to construct an [`AsStoreMut`]
/// from a [`Store`]. The internal [`StoreMut`] is taken when
/// using this value to invoke [`Function::call`](crate::Function::call).
// TODO: can we put the value back after the function returns? We should be able to
// TODO: what would the API look like?
pub struct StoreMutGuard<'a> {
    inner: Option<StoreMut>,
    marker: std::marker::PhantomData<&'a ()>,
}

impl AsStoreRef for StoreMutGuard<'_> {
    fn as_ref(&self) -> &StoreInner {
        self.inner
            .as_ref()
            .expect("StoreMutGuard is taken")
            .as_ref()
    }
}

impl AsStoreMut for StoreMutGuard<'_> {
    fn as_mut(&mut self) -> &mut StoreInner {
        self.inner
            .as_mut()
            .expect("StoreMutGuard is taken")
            .as_mut()
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        self.inner.as_mut().expect("StoreMutGuard is taken")
    }

    fn take(&mut self) -> Option<StoreMut> {
        self.inner.take()
    }

    fn put_back(&mut self, store_mut: StoreMut) {
        assert!(self.inner.replace(store_mut).is_none());
    }
}
