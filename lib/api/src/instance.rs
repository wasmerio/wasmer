use crate::exports::Exports;
use crate::externals::Extern;
use crate::module::Module;
use crate::store::Store;
use crate::InstantiationError;
use wasmer_jit::Resolver;
use wasmer_runtime::InstanceHandle;

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
#[derive(Clone)]
pub struct Instance {
    handle: InstanceHandle,
    module: Module,
    /// The exports for an instance.
    pub exports: Exports,
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports resolved by the [`Resolver`].
    ///
    /// The resolver can be anything that implements the [`Resolver`] trait,
    /// so you can plug custom resolution for the imports.
    ///
    /// The [`ImportObject`] is the easiest way to provide imports to the instance.
    ///
    /// ```
    /// let store = Store::default();
    /// let module = Module::new(store, "(module)");
    /// let imports = imports!{
    ///   "host" => {
    ///     "var" => Global::new(Value::I32(2))
    ///   }
    /// };
    /// let instance = Instance::new(&module, &imports);
    /// ```
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationErrors`].
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    pub fn new(module: &Module, resolver: &dyn Resolver) -> Result<Instance, InstantiationError> {
        let store = module.store();

        let handle = store
            .engine()
            .instantiate(module.compiled_module(), resolver)?;

        let exports = module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_export(store, export.clone());
                (name.to_string(), extern_)
            })
            .collect::<Exports>();

        Ok(Instance {
            handle,
            module: module.clone(),
            exports,
        })
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }

    pub fn store(&self) -> &Store {
        self.module.store()
    }
}
