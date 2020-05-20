use wasmer_runtime::ModuleInfo;

use downcast_rs::{impl_downcast, Downcast};

/// The `CompiledModule` trait is used by engine implementors, such
/// as a JIT or Native execution.
pub trait CompiledModule: Downcast {
    /// Return a pointer to a module.
    fn module(&self) -> &ModuleInfo;

    /// Return a mutable pointer to a module.
    fn module_mut(&mut self) -> &mut ModuleInfo;
}

impl_downcast!(CompiledModule);
