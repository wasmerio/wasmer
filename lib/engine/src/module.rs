use wasmer_runtime::Module;

use downcast_rs::{impl_downcast, Downcast};

/// The `CompiledModule` trait is used by engine implementors, such
/// as a JIT or Native execution.
pub trait CompiledModule: Downcast {
    /// Return a pointer to a module.
    fn module(&self) -> &Module;

    /// Return a mutable pointer to a module.
    fn module_mut(&mut self) -> &mut Module;
}

impl_downcast!(CompiledModule);
