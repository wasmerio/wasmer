use crate::exports::Exports;
use crate::imports::Imports;
use crate::js::as_js::AsJs;
use crate::js::error::InstantiationError;
use crate::js::externals::Extern;
use crate::js::vm::{VMExtern, VMInstance};
use crate::module::Module;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::{LinkError, RuntimeError};
use js_sys::WebAssembly;
use std::fmt;

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
    pub(crate) handle: VMInstance,
    pub(crate) module: Module,
    /// The exports for an instance.
    pub exports: Exports,
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
    /// let mut store = Store::default();
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
        mut store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        let instance = module
            .0
            .instantiate(&mut store, imports)
            .map_err(|e| InstantiationError::Start(e))?;

        let self_instance = Self::from_jsvalue(store, &module, &instance).map_err(|e| {
            let js_err: wasm_bindgen::JsValue = e.into();
            let err: RuntimeError = js_err.into();
            InstantiationError::Link(LinkError::Trap(err))
        })?;
        Ok(self_instance)
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
    pub fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<Self, InstantiationError> {
        let mut imports = Imports::new();
        for (import_ty, extern_ty) in module.imports().zip(externs.iter()) {
            imports.define(import_ty.module(), import_ty.name(), extern_ty.clone());
        }
        Self::new(store, module, &imports)
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
