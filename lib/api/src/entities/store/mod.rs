//! Defines the [`Store`] data type and various useful traits and data types to interact with a
//! store.

/// Defines the [`AsAsyncStore`] trait and its supporting types.
mod asynk;
pub use asynk::*;

/// Defines the [`StoreContext`] type.
mod context;

/// Defines the [`StoreInner`] data type.
mod inner;

/// Create temporary handles to engines.
mod store_ref;

/// Single-threaded async-aware RwLock.
mod local_rwlock;
pub(crate) use local_rwlock::*;

use std::ops::{Deref, DerefMut};

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
    pub(crate) id: StoreId,
    pub(crate) inner: LocalRwLock<StoreInner>,
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

        let objects = StoreObjects::from_store_ref(&store);
        Self {
            id: objects.id(),
            inner: LocalRwLock::new(StoreInner {
                objects,
                on_called: None,
                store,
            }),
        }
    }

    /// Clones the [`Store`]. This is a cheap operation that internally
    /// acquires a new [`Rc`](std::rc::Rc) handle.
    ///
    /// This is a dangerous (but safe) operation because, with multiple
    /// clones of the same store, it's possible (and rather easy) to
    /// run into deadlock situations. [`Store::as_ref`] and [`Store::as_mut`]
    /// artificially borrow the [`Store`] instance to prevent deadlocks.
    /// Having a second clone of the store around disables this mechanism.
    pub fn dangerous_clone(&self) -> Self {
        Self {
            id: self.id,
            inner: self.inner.clone(),
        }
    }

    /// Creates a new [`StoreRef`] if the store is available for reading.
    pub(crate) fn try_make_ref(&self) -> Option<StoreRef> {
        self.inner
            .try_read_rc()
            .map(|guard| StoreRef { inner: guard })
    }

    /// Waits for the store to become available and creates a new
    /// [`StoreRef`] afterwards.
    pub(crate) async fn make_ref_async(&self) -> StoreRef {
        let guard = self.inner.read_rc().await;
        StoreRef { inner: guard }
    }

    /// Creates a new [`StoreMut`] if the store is available for writing.
    pub(crate) fn try_make_mut(&self) -> Option<StoreMut> {
        self.inner.try_write_rc().map(|guard| StoreMut {
            inner: guard,
            store_handle: Self {
                id: self.id,
                inner: self.inner.clone(),
            },
        })
    }

    /// Waits for the store to become available and creates a new
    /// [`StoreMut`] afterwards.
    pub(crate) async fn make_mut_async(&self) -> StoreMut {
        let guard = self.inner.write_rc().await;
        StoreMut {
            inner: guard,
            store_handle: Self {
                id: self.id,
                inner: self.inner.clone(),
            },
        }
    }

    /// Builds an [`AsStoreRef`] handle to this store, provided
    /// the store is not locked for writing. Panics otherwise.
    pub fn as_ref<'a>(&'a self) -> impl AsStoreRef + 'a {
        // Note: only the FromContext variant of StoreRefGuard can cause panics,
        // so this is safe.
        StoreRefGuard::Owned {
            inner: self.try_make_ref().expect("Store is locked for writing"),
            marker: std::marker::PhantomData,
        }
    }

    /// Builds an [`AsStoreRef`] handle to this store if it's not
    /// currently locked for writing. If the store is locked,
    /// tries to acquire a reference through the store context.
    /// Panics if neither is possible.
    ///
    /// This is dangerous because it can potentially acquire
    /// a handle to the current store context. When the context is about
    /// to be uninstalled, it checks for existing handles (to prevent
    /// undefined behavior) and panics if there are live handles. Hence,
    /// the return value from this function *must* be dropped before the
    /// store context is uninstalled, which can happen at any of these times:
    ///   * When the call to a module's start function finishes during
    ///     instantiation
    ///   * When a synchronous call to a WASM function returns
    ///   * When an asynchronous WASM function suspends
    ///
    /// # Safety
    /// Calling this function potentially allows mutable and const references
    /// to the same store to exist at the same time, violating Rust's aliasing
    /// rules. Therefore, the caller must make sure no [`AsStoreMut`] referencing
    /// this store is in scope while the returned reference is alive.
    pub unsafe fn dangerous_ref_from_context<'a>(&'a self) -> impl AsStoreRef + 'a {
        if let Some(store_ref) = self.try_make_ref() {
            StoreRefGuard::Owned {
                inner: store_ref,
                marker: std::marker::PhantomData,
            }
        } else if let Some(from_context) =
            unsafe { context::StoreContext::try_get_current(self.id) }
        {
            StoreRefGuard::FromContext {
                inner: from_context,
            }
        } else {
            panic!("Store is locked for writing but not installed in the store context")
        }
    }

    /// Builds an [`AsStoreMut`] handle to this store, provided
    /// the store is not locked. Panics if the store is already locked.
    pub fn as_mut<'a>(&'a mut self) -> impl AsStoreMut + 'a {
        StoreMutGuard::Owned {
            inner: Some(self.try_make_mut().expect("Store is locked")),
            marker: std::marker::PhantomData,
        }
    }

    /// Builds an [`AsStoreMut`] handle to this store if it's not
    /// currently locked. If the store is locked, tries to acquire
    /// a reference through the store context. Panics if neither is
    /// possible.
    ///
    /// This is dangerous because it can potentially acquire
    /// a handle to the current store context. When the context is about
    /// to be uninstalled, it checks for existing handles (to prevent
    /// undefined behavior) and panics if there are live handles. Hence,
    /// the return value from this function *must* be dropped before the
    /// store context is uninstalled, which can happen at any of these times:
    ///   * When the call to a module's start function finishes during
    ///     instantiation
    ///   * When a synchronous call to a WASM function returns
    ///   * When an asynchronous WASM function suspends
    ///
    /// # Safety
    /// Calling this function potentially allows multiple mutable references
    /// to the same store to exist at the same time, violating Rust's aliasing
    /// rules. Therefore, the caller must make sure no [`AsStoreMut`] referencing
    /// this store is in scope while the returned reference is alive.
    pub unsafe fn dangerous_mut_from_context<'a>(&'a self) -> impl AsStoreMut + 'a {
        if let Some(store_mut) = self.try_make_mut() {
            StoreMutGuard::Owned {
                inner: Some(store_mut),
                marker: std::marker::PhantomData,
            }
        } else if let Some(from_context) =
            unsafe { context::StoreContext::try_get_current(self.id) }
        {
            StoreMutGuard::FromContext {
                inner: from_context,
            }
        } else {
            panic!("Store is locked for writing but not installed in the store context")
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
        if let BackendStore::Sys(ref mut s) =
            self.try_make_mut().expect("Store is locked").inner.store
        {
            s.set_trap_handler(handler)
        }
    }

    /// Returns the [`Engine`].
    pub fn engine<'a>(&'a self) -> StoreEngineRef<'a> {
        // Happily unwrap the read lock here because we don't expect
        // embedder code to access stores in parallel.
        StoreEngineRef {
            inner: self.try_make_ref().expect("Store is locked"),
            marker: std::marker::PhantomData,
        }
    }

    /// Returns mutable reference to [`Engine`].
    pub fn engine_mut<'a>(&'a mut self) -> StoreEngineMut<'a> {
        StoreEngineMut {
            inner: self.try_make_mut().expect("Store is locked"),
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
        self.id
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

/// The immutable counter-part to [`StoreMutGuard`].
pub(crate) enum StoreRefGuard<'a> {
    Owned {
        inner: StoreRef,
        marker: std::marker::PhantomData<&'a ()>,
    },
    FromContext {
        inner: context::StoreMutWrapper,
    },
}

impl AsStoreRef for StoreRefGuard<'_> {
    fn as_ref(&self) -> &StoreInner {
        match self {
            Self::Owned { inner, .. } => inner.as_ref(),
            Self::FromContext { inner } => inner.as_ref().as_ref(),
        }
    }
}

/// A guard that provides mutable access to a [`Store`]. This is
/// the only way for embedders to construct an [`AsStoreMut`]
/// from a [`Store`]. The internal [`StoreMut`] is taken when
/// using this value to invoke [`Function::call`](crate::Function::call).
// TODO: can we put the value back after the function returns? We should be able to
// TODO: what would the API look like?
pub(crate) enum StoreMutGuard<'a> {
    Owned {
        inner: Option<StoreMut>,
        marker: std::marker::PhantomData<&'a ()>,
    },
    FromContext {
        inner: context::StoreMutWrapper,
    },
}

impl AsStoreRef for StoreMutGuard<'_> {
    fn as_ref(&self) -> &StoreInner {
        match self {
            Self::Owned { inner, .. } => inner.as_ref().expect("StoreMutGuard is taken").as_ref(),
            Self::FromContext { inner } => inner.as_ref().as_ref(),
        }
    }
}

impl AsStoreMut for StoreMutGuard<'_> {
    fn as_mut(&mut self) -> &mut StoreInner {
        match self {
            Self::Owned { inner, .. } => inner.as_mut().expect("StoreMutGuard is taken").as_mut(),
            Self::FromContext { inner } => inner.as_mut().as_mut(),
        }
    }

    fn reborrow_mut(&mut self) -> &mut StoreMut {
        match self {
            Self::Owned { inner, .. } => inner.as_mut().expect("StoreMutGuard is taken"),
            Self::FromContext { inner } => inner.as_mut(),
        }
    }

    fn take(&mut self) -> Option<StoreMut> {
        match self {
            Self::Owned { inner, .. } => inner.take(),
            Self::FromContext { inner } => None,
        }
    }

    fn put_back(&mut self, store_mut: StoreMut) {
        match self {
            Self::Owned { inner, .. } => assert!(inner.replace(store_mut).is_none()),
            Self::FromContext { inner: _ } => panic!("Cannot put back into FromContext store"),
        }
    }
}
