use crate::js::context::{AsContextMut, AsContextRef, StoreHandle};
use crate::js::error::InstantiationError;
use crate::js::export::Export;
use crate::js::exports::Exports;
use crate::js::externals::Extern;
use crate::js::imports::Imports;
use crate::js::module::Module;
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
    #[allow(dead_code)]
    imports: Imports,
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
        ctx: &mut impl AsContextMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        let import_copy = imports.clone();
        let (instance, _imports): (StoreHandle<WebAssembly::Instance>, Vec<Extern>) = module
            .instantiate(&mut ctx.as_context_mut(), imports)
            .map_err(|e| InstantiationError::Start(e))?;

        let self_instance = Self::from_module_and_instance(ctx, module, instance, import_copy)?;
        //self_instance.init_envs(&imports.iter().map(Extern::to_export).collect::<Vec<_>>())?;
        Ok(self_instance)
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
        ctx: &mut impl AsContextMut,
        module: &Module,
        instance: StoreHandle<WebAssembly::Instance>,
        imports: Imports,
    ) -> Result<Self, InstantiationError> {
        let instance_exports = instance.get(store.objects()).exports();
        let exports = module
            .exports()
            .map(|export_type| {
                let name = export_type.name();
                let extern_type = export_type.ty().clone();
                let js_export =
                    js_sys::Reflect::get(&instance_exports, &name.into()).map_err(|_e| {
                        InstantiationError::Link(format!(
                            "Can't get {} from the instance exports",
                            &name
                        ))
                    })?;
                let export: Export =
                    Export::from_js_value(js_export, &mut ctx.as_context_mut(), extern_type)?
                        .into();
                let extern_ = Extern::from_vm_export(&mut ctx.as_context_mut(), export);
                Ok((name.to_string(), extern_))
            })
            .collect::<Result<Exports, InstantiationError>>()?;

        Ok(Self {
            _handle: instance,
            module: module.clone(),
            imports,
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
        ctx: &'context impl AsContextRef,
    ) -> &'context WebAssembly::Instance {
        &self._handle.get(store.objects())
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}
