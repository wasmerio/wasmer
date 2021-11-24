use crate::js::{Export, ExternType, Module, NamedResolver};
use std::collections::HashMap;

/// All of the import data used when instantiating.
///
/// It's suggested that you use the [`imports!`] macro
/// instead of creating an `ImportObject` by hand.
///
/// [`imports!`]: macro.imports.html
///
/// # Usage:
/// ```ignore
/// use wasmer::{Exports, ImportObject, Function};
///
/// let mut import_object = ImportObject::new();
/// let mut env = Exports::new();
///
/// env.insert("foo", Function::new_native(foo));
/// import_object.register("env", env);
///
/// fn foo(n: i32) -> i32 {
///     n
/// }
/// ```
#[derive(Clone, Default)]
pub struct JSObjectResolver {
    module_imports: HashMap<(String, String), ExternType>,
    object: js_sys::Object,
}

unsafe impl Send for JSObjectResolver {}
unsafe impl Sync for JSObjectResolver {}

impl JSObjectResolver {
    /// Create a new `ImportObject`.
    pub fn new(module: &Module, object: js_sys::Object) -> Self {
        let module_imports = module
            .imports()
            .map(|import| {
                (
                    (import.module().to_string(), import.name().to_string()),
                    import.ty().clone(),
                )
            })
            .collect::<HashMap<(String, String), ExternType>>();
        Self {
            module_imports,
            object,
        }
    }

    /// Gets an export given a module and a name
    ///
    /// # Usage
    /// ```ignore
    /// # use wasmer::{ImportObject, Instance, Namespace};
    /// let mut import_object = ImportObject::new();
    /// import_object.get_export("module", "name");
    /// ```
    pub fn get_export(&self, module: &str, name: &str) -> Option<Export> {
        let namespace = js_sys::Reflect::get(&self.object, &name.into()).ok()?;
        let js_export = js_sys::Reflect::get(&namespace, &name.into()).ok()?;
        match self
            .module_imports
            .get(&(module.to_string(), name.to_string()))
        {
            Some(extern_type) => Some((js_export, extern_type.clone()).into()),
            None => None,
        }
    }
}

impl Into<js_sys::Object> for JSObjectResolver {
    fn into(self) -> js_sys::Object {
        self.object
    }
}

impl NamedResolver for JSObjectResolver {
    fn resolve_by_name(&self, module: &str, name: &str) -> Option<Export> {
        self.get_export(module, name)
    }
}
