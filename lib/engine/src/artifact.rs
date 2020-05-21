use crate::{InstantiationError, RuntimeError};
use wasm_common::{DataInitializer, OwnedDataInitializer};
use wasmer_compiler::Features;
use wasmer_runtime::{InstanceHandle, ModuleInfo};

use downcast_rs::{impl_downcast, Downcast};

/// An `Artifact` is the product that the `Engine` implementation
/// produce and use.
///
/// This means, the artifact that contains the compiled information
/// for a given modue, as well as extra information needed to run the
/// module at runtime.
pub trait Artifact: Downcast {
    /// Return a pointer to a module.
    fn module(&self) -> &ModuleInfo;

    /// Return a mutable pointer to a module.
    fn module_mut(&mut self) -> &mut ModuleInfo;

    /// Finishes the instantiation of a just created `InstanceHandle`.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::finish_instantiation`
    unsafe fn finish_instantiation(
        &self,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let is_bulk_memory: bool = self.features().bulk_memory;
        let data_initializers = self
            .data_initializers()
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>();
        handle
            .finish_instantiation(is_bulk_memory, &data_initializers)
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Returns the features for this Artifact
    fn features(&self) -> &Features;

    /// Returns data initializers to pass to `InstanceHandle::initialize`
    fn data_initializers(&self) -> &Box<[OwnedDataInitializer]>;
}

impl_downcast!(Artifact);
