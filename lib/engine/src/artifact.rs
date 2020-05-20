use wasmer_runtime::ModuleInfo;

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
}

impl_downcast!(Artifact);
