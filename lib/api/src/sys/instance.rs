use crate::sys::exports::Exports;
use crate::sys::externals::Extern;
use crate::sys::imports::Imports;
use crate::sys::module::Module;
use crate::sys::{LinkError, RuntimeError};
use std::fmt;
use thiserror::Error;
use wasmer_vm::{InstanceHandle, StoreHandle};

use super::store::AsStoreMut;

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
#[derive(Clone)]
pub struct Instance {
    _handle: StoreHandle<InstanceHandle>,
    module: Module,
    /// The exports for an instance.
    pub exports: Exports,
}

#[cfg(test)]
mod send_test {
    use super::*;

    fn is_send<T: Send>() -> bool {
        true
    }

    #[test]
    fn instance_is_send() {
        assert!(is_send::<Instance>());
    }
}

/// An error while instantiating a module.
///
/// This is not a common WebAssembly error, however
/// we need to differentiate from a `LinkError` (an error
/// that happens while linking, on instantiation), a
/// Trap that occurs when calling the WebAssembly module
/// start function, and an error when initializing the user's
/// host environments.
#[derive(Error, Debug)]
pub enum InstantiationError {
    /// A linking ocurred during instantiation.
    #[error(transparent)]
    Link(LinkError),

    /// A runtime error occured while invoking the start function
    #[error(transparent)]
    Start(RuntimeError),

    /// The module was compiled with a CPU feature that is not available on
    /// the current host.
    #[error("missing required CPU features: {0:?}")]
    CpuFeature(String),

    /// Import from a different Store.
    /// This error occurs when an import from a different store is used.
    #[error("cannot mix imports from different stores")]
    DifferentStores,
}

impl From<wasmer_compiler::InstantiationError> for InstantiationError {
    fn from(other: wasmer_compiler::InstantiationError) -> Self {
        match other {
            wasmer_compiler::InstantiationError::Link(e) => Self::Link(e),
            wasmer_compiler::InstantiationError::Start(e) => Self::Start(e),
            wasmer_compiler::InstantiationError::CpuFeature(e) => Self::CpuFeature(e),
        }
    }
}

impl Instance {
    #[cfg(feature = "compiler")]
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports using [`Imports`] or the [`imports`] macro helper.
    ///
    /// [`imports`]: crate::imports
    /// [`Imports`]: crate::Imports
    ///
    /// ```
    /// # use wasmer::{imports, Store, Module, Global, Value, Instance};
    /// # use wasmer::FunctionEnv;
    /// # fn main() -> anyhow::Result<()> {
    /// let mut store = Store::default();
    /// let env = FunctionEnv::new(&mut store, ());
    /// let module = Module::new(&store, "(module)")?;
    /// let imports = imports!{
    ///   "host" => {
    ///     "var" => Global::new(&mut store, Value::I32(2))
    ///   }
    /// };
    /// let instance = Instance::new(&mut store, &module, &imports)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    pub fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        let imports = imports
            .imports_for_module(module)
            .map_err(InstantiationError::Link)?;
        let mut handle = module.instantiate(store, &imports)?;
        let exports = module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_extern(store, export);
                (name, extern_)
            })
            .collect::<Exports>();

        let instance = Self {
            _handle: StoreHandle::new(store.objects_mut(), handle),
            module: module.clone(),
            exports,
        };

        Ok(instance)
    }

    #[cfg(feature = "compiler")]
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// vector of imports.
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    pub fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<Self, InstantiationError> {
        let imports = externs.to_vec();
        let mut handle = module.instantiate(store, &imports)?;
        let exports = module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_extern(store, export);
                (name, extern_)
            })
            .collect::<Exports>();

        let instance = Self {
            _handle: StoreHandle::new(store.objects_mut(), handle),
            module: module.clone(),
            exports,
        };

        Ok(instance)
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}
