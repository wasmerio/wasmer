use crate::error::InstantiationError;
use std::sync::Arc;
use wasmer_runtime::InstanceHandle;
use wasmer_runtime::Module;

use downcast_rs::{impl_downcast, Downcast};

/// The `CompiledModule` trait is used by engine implementors, such
/// as a JIT or Native execution.
pub trait CompiledModule: Downcast {
    /// Finish instantiation of a `InstanceHandle`
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::finish_instantiation`
    unsafe fn finish_instantiation(
        &self,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError>;

    /// Return a reference-counting pointer to a module.
    fn module(&self) -> &Module;

    /// Return a reference-counting pointer to a module.
    fn module_mut(&mut self) -> &mut Module;
}

impl_downcast!(CompiledModule); // `sync` => also produce `Arc` downcasts.
