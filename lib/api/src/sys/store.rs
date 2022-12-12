use crate::sys::tunables::BaseTunables;
use std::fmt;
use std::sync::{Arc, RwLock};
#[cfg(feature = "compiler")]
use wasmer_compiler::{AsEngineRef, Engine, EngineBuilder, EngineRef, Tunables};
use wasmer_vm::{init_traps, TrapHandler, TrapHandlerFn};

use wasmer_vm::StoreObjects;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    #[cfg(feature = "compiler")]
    pub(crate) engine: Engine,
    pub(crate) trap_handler: Option<Box<TrapHandlerFn<'static>>>,
}

/// The store represents all global state that can be manipulated by
/// WebAssembly programs. It consists of the runtime representation
/// of all instances of functions, tables, memories, and globals that
/// have been allocated during the lifetime of the abstract machine.
///
/// The `Store` holds the engine (that is —amongst many things— used to compile
/// the Wasm bytes into a valid module artifact).
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#store>
pub struct Store {
    pub(crate) inner: Box<StoreInner>,
    #[cfg(feature = "compiler")]
    engine: Engine,
    trap_handler: Arc<RwLock<Option<Box<TrapHandlerFn<'static>>>>>,
}

impl Store {
    #[cfg(feature = "compiler")]
    /// Creates a new `Store` with a specific [`Engine`].
    pub fn new(engine: impl Into<Engine>) -> Self {
        let engine = engine.into();

        // Make sure the signal handlers are installed.
        // This is required for handling traps.
        init_traps();

        Self {
            inner: Box::new(StoreInner {
                objects: Default::default(),
                engine: engine.cloned(),
                trap_handler: None,
            }),
            engine: engine.cloned(),
            trap_handler: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(feature = "compiler")]
    #[deprecated(
        since = "3.0.0",
        note = "Store::new_with_engine has been deprecated in favor of Store::new"
    )]
    /// Creates a new `Store` with a specific [`Engine`].
    pub fn new_with_engine(engine: impl Into<Engine>) -> Self {
        Self::new(engine)
    }

    /// Set the trap handler in this store.
    pub fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        self.inner.trap_handler = handler;
    }

    #[cfg(feature = "compiler")]
    /// Creates a new `Store` with a specific [`Engine`] and [`Tunables`].
    pub fn new_with_tunables(
        engine: impl Into<Engine>,
        tunables: impl Tunables + Send + Sync + 'static,
    ) -> Self {
        let mut engine = engine.into();
        engine.set_tunables(tunables);

        Self::new(engine)
    }

    #[cfg(feature = "compiler")]
    /// Returns the [`Tunables`].
    pub fn tunables(&self) -> &dyn Tunables {
        self.engine.tunables()
    }

    #[cfg(feature = "compiler")]
    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    #[cfg(feature = "compiler")]
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.engine.id() == b.engine.id()
    }
}

#[cfg(feature = "compiler")]
impl PartialEq for Store {
    fn eq(&self, other: &Self) -> bool {
        Self::same(self, other)
    }
}

unsafe impl TrapHandler for Store {
    fn custom_trap_handler(&self, call: &dyn Fn(&TrapHandlerFn) -> bool) -> bool {
        if let Some(handler) = self.trap_handler.read().unwrap().as_ref() {
            call(handler)
        } else {
            false
        }
    }
}

// impl PartialEq for Store {
//     fn eq(&self, other: &Self) -> bool {
//         Self::same(self, other)
//     }
// }

// This is required to be able to set the trap_handler in the
// Store.
unsafe impl Send for Store {}
unsafe impl Sync for Store {}

// We only implement default if we have assigned a default compiler and engine
#[cfg(feature = "compiler")]
impl Default for Store {
    fn default() -> Self {
        // We store them on a function that returns to make
        // sure this function doesn't emit a compile error even if
        // more than one compiler is enabled.
        #[allow(unreachable_code)]
        #[cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]
        fn get_config() -> impl wasmer_compiler::CompilerConfig + 'static {
            cfg_if::cfg_if! {
                if #[cfg(feature = "cranelift")] {
                    wasmer_compiler_cranelift::Cranelift::default()
                } else if #[cfg(feature = "llvm")] {
                    wasmer_compiler_llvm::LLVM::default()
                } else if #[cfg(feature = "singlepass")] {
                    wasmer_compiler_singlepass::Singlepass::default()
                } else {
                    compile_error!("No default compiler chosen")
                }
            }
        }

        #[allow(unreachable_code, unused_mut)]
        fn get_engine() -> Engine {
            cfg_if::cfg_if! {
                if #[cfg(feature = "compiler")] {

            cfg_if::cfg_if! {
                    if #[cfg(any(feature = "cranelift", feature = "llvm", feature = "singlepass"))]
                    {
                    let config = get_config();
                    EngineBuilder::new(Box::new(config) as Box<dyn wasmer_compiler::CompilerConfig>)
                        .engine()
                    } else {
                    EngineBuilder::headless()
                        .engine()
                    }
            }
                } else {
                    compile_error!("No default engine chosen")
                }
            }
        }

        let engine = get_engine();
        let tunables = BaseTunables::for_target(engine.target());
        Self::new_with_tunables(&engine, tunables)
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

#[cfg(feature = "compiler")]
impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}

#[cfg(feature = "compiler")]
impl AsEngineRef for &Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}

#[cfg(feature = "compiler")]
impl AsEngineRef for StoreRef<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.inner.engine)
    }
}

#[cfg(feature = "compiler")]
impl AsEngineRef for StoreMut<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.inner.engine)
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Store").finish()
    }
}

/// A temporary handle to a [`Store`].
pub struct StoreRef<'a> {
    pub(crate) inner: &'a StoreInner,
}

impl<'a> StoreRef<'a> {
    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }

    #[cfg(feature = "compiler")]
    /// Returns the [`Tunables`].
    pub fn tunables(&self) -> &dyn Tunables {
        self.inner.engine.tunables()
    }

    #[cfg(feature = "compiler")]
    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    #[cfg(feature = "compiler")]
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.engine.id() == b.inner.engine.id()
    }

    /// The signal handler
    #[inline]
    pub fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        self.inner
            .trap_handler
            .as_ref()
            .map(|handler| handler as *const _)
    }
}

/// A temporary handle to a [`Store`].
pub struct StoreMut<'a> {
    pub(crate) inner: &'a mut StoreInner,
}

impl<'a> StoreMut<'a> {
    /// Returns the [`Tunables`].
    #[cfg(feature = "compiler")]
    pub fn tunables(&self) -> &dyn Tunables {
        self.inner.engine.tunables()
    }

    /// Returns the [`Engine`].
    #[cfg(feature = "compiler")]
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    #[cfg(feature = "compiler")]
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.engine.id() == b.inner.engine.id()
    }

    #[cfg(feature = "compiler")]
    pub(crate) fn tunables_and_objects_mut(&mut self) -> (&dyn Tunables, &mut StoreObjects) {
        (self.inner.engine.tunables(), &mut self.inner.objects)
    }

    pub(crate) fn as_raw(&self) -> *mut StoreInner {
        self.inner as *const StoreInner as *mut StoreInner
    }

    pub(crate) unsafe fn from_raw(raw: *mut StoreInner) -> Self {
        Self { inner: &mut *raw }
    }
}

/// Helper trait for a value that is convertible to a [`StoreRef`].
pub trait AsStoreRef {
    /// Returns a `StoreRef` pointing to the underlying context.
    fn as_store_ref(&self) -> StoreRef<'_>;
}

/// Helper trait for a value that is convertible to a [`StoreMut`].
pub trait AsStoreMut: AsStoreRef {
    /// Returns a `StoreMut` pointing to the underlying context.
    fn as_store_mut(&mut self) -> StoreMut<'_>;

    /// Returns the ObjectMutable
    fn objects_mut(&mut self) -> &mut StoreObjects;
}

impl AsStoreRef for StoreRef<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}

impl AsStoreRef for StoreMut<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        StoreRef { inner: self.inner }
    }
}
impl AsStoreMut for StoreMut<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        StoreMut { inner: self.inner }
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        &mut self.inner.objects
    }
}

impl<T: AsStoreRef> AsStoreRef for &'_ T {
    fn as_store_ref(&self) -> StoreRef<'_> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreRef> AsStoreRef for &'_ mut T {
    fn as_store_ref(&self) -> StoreRef<'_> {
        T::as_store_ref(*self)
    }
}
impl<T: AsStoreMut> AsStoreMut for &'_ mut T {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        T::as_store_mut(*self)
    }
    fn objects_mut(&mut self) -> &mut StoreObjects {
        T::objects_mut(*self)
    }
}
