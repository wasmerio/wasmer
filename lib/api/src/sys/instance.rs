use crate::sys::exports::Exports;
use crate::sys::externals::Extern;
use crate::sys::imports::Imports;
use crate::sys::module::Module;
use crate::sys::store::Store;
use crate::sys::{HostEnvInitError, LinkError, RuntimeError};
use std::fmt;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use wasmer_vm::{InstanceHandle, VMContext};

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
    handle: Arc<Mutex<InstanceHandle>>,
    module: Module,
    #[allow(dead_code)]
    imports: Vec<Extern>,
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
    #[error("missing requires CPU features: {0:?}")]
    CpuFeature(String),

    /// Error occurred when initializing the host environment.
    #[error(transparent)]
    HostEnvInitialization(HostEnvInitError),
}

impl From<wasmer_engine::InstantiationError> for InstantiationError {
    fn from(other: wasmer_engine::InstantiationError) -> Self {
        match other {
            wasmer_engine::InstantiationError::Link(e) => Self::Link(e),
            wasmer_engine::InstantiationError::Start(e) => Self::Start(e),
            wasmer_engine::InstantiationError::CpuFeature(e) => Self::CpuFeature(e),
        }
    }
}

impl From<HostEnvInitError> for InstantiationError {
    fn from(other: HostEnvInitError) -> Self {
        Self::HostEnvInitialization(other)
    }
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports using [`Imports`] or the [`imports`] macro helper.
    ///
    /// [`imports`]: crate::imports
    /// [`Imports`]: crate::Imports
    ///
    /// ```
    /// # use wasmer::{imports, Store, Module, Global, Value, Instance};
    /// # fn main() -> anyhow::Result<()> {
    /// let store = Store::default();
    /// let module = Module::new(&store, "(module)")?;
    /// let imports = imports!{
    ///   "host" => {
    ///     "var" => Global::new(&store, Value::I32(2))
    ///   }
    /// };
    /// let instance = Instance::new(&module, &imports)?;
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
    pub fn new(module: &Module, imports: &Imports) -> Result<Self, InstantiationError> {
        let store = module.store();
        let imports = imports
            .imports_for_module(module)
            .map_err(InstantiationError::Link)?;
        let handle = module.instantiate(&imports)?;
        let exports = module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_export(store, export.into());
                (name, extern_)
            })
            .collect::<Exports>();

        let instance = Self {
            handle: Arc::new(Mutex::new(handle)),
            module: module.clone(),
            imports,
            exports,
        };

        // # Safety
        // `initialize_host_envs` should be called after instantiation but before
        // returning an `Instance` to the user. We set up the host environments
        // via `WasmerEnv::init_with_instance`.
        //
        // This usage is correct because we pass a valid pointer to `instance` and the
        // correct error type returned by `WasmerEnv::init_with_instance` as a generic
        // parameter.
        unsafe {
            instance
                .handle
                .lock()
                .unwrap()
                .initialize_host_envs::<HostEnvInitError>(&instance as *const _ as *const _)?;
        }

        Ok(instance)
    }

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
    pub fn new_by_index(module: &Module, externs: &[Extern]) -> Result<Self, InstantiationError> {
        let store = module.store();
        let imports = externs.iter().cloned().collect::<Vec<_>>();
        let handle = module.instantiate(&imports)?;
        let exports = module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_export(store, export.into());
                (name, extern_)
            })
            .collect::<Exports>();

        let instance = Self {
            handle: Arc::new(Mutex::new(handle)),
            module: module.clone(),
            imports,
            exports,
        };

        // # Safety
        // `initialize_host_envs` should be called after instantiation but before
        // returning an `Instance` to the user. We set up the host environments
        // via `WasmerEnv::init_with_instance`.
        //
        // This usage is correct because we pass a valid pointer to `instance` and the
        // correct error type returned by `WasmerEnv::init_with_instance` as a generic
        // parameter.
        unsafe {
            instance
                .handle
                .lock()
                .unwrap()
                .initialize_host_envs::<HostEnvInitError>(&instance as *const _ as *const _)?;
        }

        Ok(instance)
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Returns the [`Store`] where the `Instance` belongs.
    pub fn store(&self) -> &Store {
        self.module.store()
    }

    #[doc(hidden)]
    pub fn vmctx_ptr(&self) -> *mut VMContext {
        self.handle.lock().unwrap().vmctx_ptr()
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}
