use crate::js::error::InstantiationError;
use crate::js::export::Export;
use crate::js::exports::Exports;
use crate::js::externals::Extern;
use crate::js::imports::Imports;
use crate::js::module::Module;
use crate::js::store::{AsStoreMut, AsStoreRef, StoreHandle};
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
    _handle: StoreHandle<WebAssembly::Instance>,
    module: Module,
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
        let instance: WebAssembly::Instance = module
            .instantiate(&mut store, imports)
            .map_err(|e| InstantiationError::Start(e))?;

        let self_instance = Self::from_module_and_instance(store, module, instance)?;
        //self_instance.init_envs(&imports.iter().map(Extern::to_export).collect::<Vec<_>>())?;
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
        mut store: &mut impl AsStoreMut,
        module: &Module,
        instance: WebAssembly::Instance,
    ) -> Result<Self, InstantiationError> {
        use crate::js::externals::VMExtern;
        let instance_exports = instance.exports();
        let exports = module
            .exports()
            .map(|export_type| {
                let name = export_type.name();
                let extern_type = export_type.ty().clone();
                let js_export = js_sys::Reflect::get(&instance_exports, &name.into())
                    .map_err(|_e| InstantiationError::NotInExports(name.to_string()))?;
                let export: VMExtern =
                    VMExtern::from_js_value(js_export, &mut store, extern_type)?.into();
                let extern_ = Extern::from_vm_extern(&mut store, export);
                Ok((name.to_string(), extern_))
            })
            .collect::<Result<Exports, InstantiationError>>()?;
        let handle = StoreHandle::new(store.as_store_mut().objects_mut(), instance);
        Ok(Self {
            _handle: handle,
            module: module.clone(),
            exports,
        })
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Returns the inner WebAssembly Instance
    #[doc(hidden)]
    pub fn raw<'context>(
        &self,
        store: &'context impl AsStoreRef,
    ) -> &'context WebAssembly::Instance {
        &self._handle.get(store.as_store_ref().objects())
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}
