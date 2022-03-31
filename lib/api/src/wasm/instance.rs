use crate::wasm::env::HostEnvInitError;
use crate::wasm::export::Export;
use crate::wasm::exports::Exports;
use crate::wasm::module::Module;
use crate::wasm::resolver::Resolver;
use crate::wasm::store::Store;
use std::fmt;
#[cfg(feature = "std")]
use thiserror::Error;
use wasmer_engine::{LinkError, RuntimeError};
use wasmer_fakevm::InstanceHandle;

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
pub struct Instance {
    instance: InstanceHandle,
    module: Module,
    /// The exports for an instance.
    pub exports: Exports,
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

#[cfg(feature = "core")]
impl std::fmt::Display for InstantiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstantiationError")
    }
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports resolved by the [`Resolver`].
    ///
    /// The resolver can be anything that implements the [`Resolver`] trait,
    /// so you can plug custom resolution for the imports, if you wish not
    /// to use [`ImportObject`].
    ///
    /// The [`ImportObject`] is the easiest way to provide imports to the instance.
    ///
    /// [`ImportObject`]: crate::js::ImportObject
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
    pub fn new(
        _module: &Module,
        _resolver: &(dyn Resolver + Send + Sync),
    ) -> Result<Self, InstantiationError> {
        panic!("Not implemented!")
    }

    /// Creates a Wasmer `Instance` from a Wasmer `Module` and a WebAssembly Instance
    ///
    /// # Important
    ///
    /// Is expected that the function [`Instance::init_envs`] is run manually
    /// by the user in case the instance has any Wasmer imports, so the function
    /// environments are properly initiated.
    ///
    /// *This method is only available when targeting JS environments*
    pub fn from_module_and_instance(
        _module: &Module,
        _instance: InstanceHandle,
    ) -> Result<Self, InstantiationError> {
        panic!("Not implemented!")
    }

    /// Initialize the given extern imports with the `Instance`.
    ///
    /// # Important
    ///
    /// This method should be called if the Wasmer `Instance` is initialized
    /// from Javascript with an already existing `WebAssembly.Instance` but with
    /// a imports from the Rust side.
    ///
    /// *This method is only available when targeting JS environments*
    pub fn init_envs(&self, _imports: &[Export]) -> Result<(), InstantiationError> {
        panic!("Not implemented!")
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Returns the [`Store`] where the `Instance` belongs.
    pub fn store(&self) -> &Store {
        self.module.store()
    }

    /// Returns the inner WebAssembly Instance
    #[doc(hidden)]
    pub fn raw(&self) -> &InstanceHandle {
        &self.instance
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}
