use std::sync::Arc;
use wasmer_runtime::Module;

use downcast_rs::{DowncastSync, impl_downcast};

/// The `CompiledModule` trait is used by engine implementors, such
/// as a JIT or Native execution.
pub trait CompiledModule: DowncastSync {
    /// Return a reference-counting pointer to a module.
    fn module(&self) -> &Arc<Module>;

    /// Return a reference-counting pointer to a module.
    fn module_mut(&mut self) -> &mut Arc<Module>;

    /// Return a reference to a module.
    fn module_ref(&self) -> &Module;
}

impl_downcast!(sync CompiledModule);  // `sync` => also produce `Arc` downcasts.
