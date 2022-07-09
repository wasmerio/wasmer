use crate::sys::tunables::BaseTunables;
use crate::FunctionEnv;
use std::fmt;
use std::sync::{Arc, RwLock};
use wasmer_compiler::CompilerConfig;
use wasmer_compiler::{Engine, Tunables, Universal};
use wasmer_vm::{init_traps, TrapHandler, TrapHandlerFn};

use wasmer_vm::StoreObjects;

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    pub(crate) engine: Arc<dyn Engine + Send + Sync>,
    pub(crate) tunables: Box<dyn Tunables + Send + Sync>,
    pub(crate) trap_handler: Option<Box<TrapHandlerFn<'static>>>,
}

/// The store represents all global state that can be manipulated by
/// WebAssembly programs. It consists of the runtime representation
/// of all instances of functions, tables, memories, and globals that
/// have been allocated during the lifetime of the abstract machine.
///
/// The `Store` holds the engine (that is —amongst many things— used to compile
/// the Wasm bytes into a valid module artifact), in addition to the
/// [`Tunables`] (that are used to create the memories, tables and globals).
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#store>
pub struct Store {
    pub(crate) inner: Box<StoreInner>,
}

impl Store {
    /// Creates a new `Store` with a specific [`CompilerConfig`].
    pub fn new(compiler_config: Box<dyn CompilerConfig>) -> Self {
        let engine = Universal::new(compiler_config).engine();
        Self::new_with_tunables(&engine, BaseTunables::for_target(engine.target()))
    }

    /// Creates a new `Store` with a specific [`Engine`].
    pub fn new_with_engine<E>(engine: &E) -> Self
    where
        E: Engine + ?Sized,
    {
        Self::new_with_tunables(engine, BaseTunables::for_target(engine.target()))
    }

    /// Set the trap handler in this store.
    pub fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        self.inner.trap_handler = handler;
    }

    /// Creates a new `Store` with a specific [`Engine`] and [`Tunables`].
    pub fn new_with_tunables<E>(engine: &E, tunables: impl Tunables + Send + Sync + 'static) -> Self
    where
        E: Engine + ?Sized,
    {
        // Make sure the signal handlers are installed.
        // This is required for handling traps.
        init_traps();

        Self {
            inner: Box::new(StoreInner {
                objects: Default::default(),
                engine: engine.cloned(),
                tunables: Box::new(tunables),
                trap_handler: None,
            }),
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
#[cfg(all(feature = "default-compiler", feature = "default-engine"))]
impl Default for Store {
    fn default() -> Self {
        // We store them on a function that returns to make
        // sure this function doesn't emit a compile error even if
        // more than one compiler is enabled.
        #[allow(unreachable_code)]
        fn get_config() -> impl CompilerConfig + 'static {
            cfg_if::cfg_if! {
                if #[cfg(feature = "default-cranelift")] {
                    wasmer_compiler_cranelift::Cranelift::default()
                } else if #[cfg(feature = "default-llvm")] {
                    wasmer_compiler_llvm::LLVM::default()
                } else if #[cfg(feature = "default-singlepass")] {
                    wasmer_compiler_singlepass::Singlepass::default()
                } else {
                    compile_error!("No default compiler chosen")
                }
            }
        }

        #[allow(unreachable_code, unused_mut)]
        fn get_engine(mut config: impl CompilerConfig + 'static) -> impl Engine + Send + Sync {
            cfg_if::cfg_if! {
                if #[cfg(feature = "default-universal")] {
                    wasmer_compiler::Universal::new(config)
                        .engine()
                } else {
                    compile_error!("No default engine chosen")
                }
            }
        }

        let config = get_config();
        let engine = get_engine(config);
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

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Store").finish()
    }
}

/// A temporary handle to a [`Context`].
pub struct StoreRef<'a> {
    pub(crate) inner: &'a StoreInner,
}

impl<'a> StoreRef<'a> {
    pub(crate) fn objects(&self) -> &'a StoreObjects {
        &self.inner.objects
    }

    /// Returns the [`Tunables`].
    pub fn tunables(&self) -> &dyn Tunables {
        self.inner.tunables.as_ref()
    }

    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Arc<dyn Engine + Send + Sync> {
        &self.inner.engine
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.engine.id() == b.inner.engine.id()
    }

    /// The signal handler
    #[inline]
    pub fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        if let Some(handler) = self.inner.trap_handler.as_ref() {
            Some(&*handler as *const _)
        } else {
            None
        }
    }
}

/// A temporary handle to a [`Context`].
pub struct StoreMut<'a> {
    pub(crate) inner: &'a mut StoreInner,
}

impl<'a> StoreMut<'a> {
    /// Returns the [`Tunables`].
    pub fn tunables(&self) -> &dyn Tunables {
        self.inner.tunables.as_ref()
    }

    /// Returns the [`Engine`].
    pub fn engine(&self) -> &Arc<dyn Engine + Send + Sync> {
        &self.inner.engine
    }

    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine. The
    /// tunables are excluded from the logic.
    pub fn same(a: &Self, b: &Self) -> bool {
        a.inner.engine.id() == b.inner.engine.id()
    }

    pub(crate) fn tunables_and_objects_mut(&mut self) -> (&dyn Tunables, &mut StoreObjects) {
        (self.inner.tunables.as_ref(), &mut self.inner.objects)
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
