use crate::{
    entities::{
        engine::{AsEngineRef, Engine},
        store::StoreMut,
    }, view::MemoryViewCreator, vm::{VMExternRefCreator, VMExternRefResolver, VMFuncRefCreator, VMFuncRefResolver}, AsStoreMut, ExternRefCreator, ExternRefLike, ExternRefResolver, GlobalCreator, MemoryCreator, TableCreator
};
use wasmer_vm::{StoreObjects, TrapHandlerFn};

/// We require the context to have a fixed memory address for its lifetime since
/// various bits of the VM have raw pointers that point back to it. Hence we
/// wrap the actual context in a box.
#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub(crate) struct StoreInner {
    pub(crate) objects: StoreObjects,
    #[derivative(Debug = "ignore")]
    pub(crate) store: Box<dyn StoreLike>,
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

/// The trait that every concrete store must implement.
pub trait StoreLike:
    std::fmt::Debug
    + AsEngineRef
    + MemoryViewCreator
    + MemoryCreator
    + GlobalCreator
    + TableCreator
    + VMExternRefCreator
    + VMFuncRefCreator
    + ExternRefResolver
    + VMFuncRefResolver
    + VMExternRefResolver
{
    /// Create a new [`StoreLike`] from an [`Engine`].
    fn new(engine: impl Into<Engine>) -> Self
    where
        Self: Sized;

    /// Set the [`TrapHandlerFn`] for this store.
    ///
    /// # Note
    ///
    /// Not every implementor allows changing the trap handler. In those store that
    /// don't allow it, this function has no effect.
    // [todo] xdoardo: list the implementers in the docs above.
    fn set_trap_handler(&mut self, handler: Option<Box<TrapHandlerFn<'static>>>) {
        _ = handler;
    }

    /// Retrieve the current [`TrapHandlerFn`] for this store.
    ///
    /// # Note
    ///
    /// Not every implementor allows changing the trap handler. In those store that
    /// don't allow it, this function returns [`None`]. Of course, the same happens even if the
    /// store supports setting a trap handler, but none was set.
    // [todo] xdoardo: list the implementers in the docs above.
    fn signal_handler(&self) -> Option<*const TrapHandlerFn<'static>> {
        None
    }

    /// Retrieve a reference to the [`Engine`] underlying this store.
    fn engine(&self) -> &Engine;

    /// Retrieve a mutable reference to the [`Engine`] underlying this store.
    fn engine_mut(&mut self) -> &mut Engine;

    #[cfg(feature = "sys")]
    /// Try to downcast this store as a concrete store from the "sys" embedder.
    fn as_sys(&self) -> Option<&crate::embedders::sys::entitites::store::Store> {
        None
    }

    #[cfg(feature = "sys")]
    /// Try to downcast this store as a concrete store from the "sys" embedder.
    fn as_sys_mut(&mut self) -> Option<&mut crate::embedders::sys::entitites::store::Store> {
        None
    }
}
