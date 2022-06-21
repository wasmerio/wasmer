use crate::js::context::AsContextMut;
use crate::js::error::WasmError;
use crate::js::{Export, ExternType, Module};
use std::collections::HashMap;

/// This struct is used in case you want to create an `Instance`
/// of a `Module` with imports that are provided directly from
/// Javascript with a JS Object.
#[derive(Clone, Default)]
pub struct JsImportObject {
    module_imports: HashMap<(String, String), ExternType>,
    object: js_sys::Object,
}

/// JS Objects with wasm-bindgen are not currently Send/Sync (although they
/// are in Javascript, since we can use them safely between webworkers).
unsafe impl Send for JsImportObject {}
unsafe impl Sync for JsImportObject {}

impl JsImportObject {
    /// Create a new `JsImportObject`, it receives a reference to a `Module` to
    /// map and assign the types of each import and the JS Object
    /// that contains the values of imports.
    ///
    /// # Usage
    /// ```ignore
    /// # use wasmer::JsImportObject;
    /// let import_object = JsImportObject::new(&module, js_object);
    /// ```
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
    /// # use wasmer::JsImportObject;
    /// let import_object = JsImportObject::new(&module, js_object);
    /// import_object.get_export("module", "name");
    /// ```
    pub fn get_export(
        &self,
        ctx: &mut impl AsContextMut,
        module: &str,
        name: &str,
    ) -> Result<Export, WasmError> {
        let namespace = js_sys::Reflect::get(&self.object, &module.into())?;
        let js_export = js_sys::Reflect::get(&namespace, &name.into())?;
        match self
            .module_imports
            .get(&(module.to_string(), name.to_string()))
        {
            Some(extern_type) => Ok(Export::from_js_value(js_export, ctx, extern_type.clone())?),
            None => Err(WasmError::Generic(format!(
                "Name {} not found in module {}",
                name, module
            ))),
        }
    }
}

impl Into<js_sys::Object> for JsImportObject {
    fn into(self) -> js_sys::Object {
        self.object
    }
}
