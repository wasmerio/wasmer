use crate::exports::Exports;
use crate::module::Module;
use crate::{Extern, InstantiationError};
use std::fmt;

use crate::imports::Imports;
use crate::store::AsStoreMut;

#[cfg(feature = "js")]
use crate::js::instance as instance_imp;
#[cfg(feature = "jsc")]
use crate::jsc::instance as instance_imp;
#[cfg(feature = "sys")]
use crate::sys::instance as instance_imp;

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
#[derive(Clone, PartialEq, Eq)]
pub struct Instance {
    pub(crate) _inner: instance_imp::Instance,
    pub(crate) module: Module,
    /// The exports for an instance.
    pub exports: Exports,
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports using [`Imports`] or the [`imports!`] macro helper.
    ///
    /// [`imports!`]: crate::imports!
    /// [`Imports!`]: crate::Imports!
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
    #[allow(clippy::result_large_err)]
    pub fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        Self::new_ex(store, module, imports, &InstantiationConfig::default())
    }

    /// Same as [new](Instance::new), but accepts additional configuration
    /// that will be applied when instantiating the module.
    #[allow(clippy::result_large_err)]
    pub fn new_ex(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
        config: &InstantiationConfig,
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) = instance_imp::Instance::new(store, module, imports, config)?;
        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
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
    #[allow(clippy::result_large_err)]
    pub fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<Self, InstantiationError> {
        Self::new_by_index_ex(store, module, externs, &InstantiationConfig::default())
    }

    /// Same as [new_by_index](Instance::new_by_index), but accepts additional
    /// configuration that will be applied when instantiating the module.
    #[allow(clippy::result_large_err)]
    pub fn new_by_index_ex(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
        config: &InstantiationConfig,
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) =
            instance_imp::Instance::new_by_index(store, module, externs, config)?;
        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
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

/// Additional configuration to control the module instantiation process.
pub struct InstantiationConfig {
    pub(crate) apply_data_initializers: bool,
}

impl Default for InstantiationConfig {
    fn default() -> Self {
        Self {
            apply_data_initializers: true,
        }
    }
}

impl InstantiationConfig {
    /// Create a new, default instance of [`InstantiationConfig`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether to apply data initializers (i.e. active data segments) when
    /// instantiating the module. Defaults to true.
    ///
    /// Not applying data initializers can be useful when a pre-initialized
    /// memory is provided to the instance, which should not have its data
    /// overwritten.
    pub fn with_apply_data_initializers(mut self, apply_data_initializers: bool) -> Self {
        self.apply_data_initializers = apply_data_initializers;
        self
    }
}
