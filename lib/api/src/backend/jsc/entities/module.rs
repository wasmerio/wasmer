use std::path::Path;

use bytes::Bytes;
use rusty_jsc::{JSObject, JSValue};
use tracing::warn;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ImportType, ImportsIterator,
    ModuleInfo, SerializeError,
};

use crate::{
    AsEngineRef, AsStoreMut, AsStoreRef, BackendModule, Imports, InstantiationError, IntoBytes,
    RuntimeError,
    jsc::{utils::convert::AsJsc, vm::VMInstance},
};

use super::engine::IntoJSC;

#[derive(Clone, PartialEq, Eq)]
pub struct Module {
    module: JSObject,
    name: Option<String>,
    raw_bytes: Option<Bytes>,
    info: ModuleInfo,
}

// Module implements `structuredClone` in js, so it's safe it to make it Send.
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// ```js
// const module = new WebAssembly.Module(new Uint8Array([
//   0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00
// ]));
// structuredClone(module)
// ```
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

impl Module {
    pub(crate) fn from_binary(
        _engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        unsafe { Self::from_binary_unchecked(_engine, binary) }
    }

    pub(crate) unsafe fn from_binary_unchecked(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        let mut binary = binary.to_vec();
        let engine = engine.as_engine_ref();
        let jsc = engine.jsc();
        let context = jsc.context();
        let bytes = JSObject::create_typed_array_with_bytes(context, &mut binary).unwrap();
        let module_type = jsc.wasm_module_type();
        let global_wasm = jsc.global_wasm();
        let module = module_type
            .construct(context, &[bytes.to_jsvalue()])
            .map_err(|e| CompileError::Validate(format!("{}", e.to_string(context).unwrap())))?;

        Ok(unsafe { Self::from_js_module(module, binary) })
    }

    /// Creates a new WebAssembly module skipping any kind of validation from a javascript module
    ///
    pub(crate) unsafe fn from_js_module(module: JSObject, binary: impl IntoBytes) -> Self {
        let binary = binary.into_bytes();

        // The module is now validated, so we can safely parse it's types
        let info = crate::utils::polyfill::translate_module(&binary[..])
            .unwrap()
            .info;

        Self {
            module,
            name: info.name.clone(),
            raw_bytes: Some(binary.into_bytes()),
            info,
        }
    }

    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        let engine = engine.as_engine_ref();
        let jsc = engine.jsc();
        let context = jsc.context();
        let mut binary = binary.to_vec();
        let bytes = JSObject::create_typed_array_with_bytes(context, &mut binary).unwrap();

        let global_wasm = jsc.global_wasm();
        let validate_type = jsc.wasm_validate_type();

        match validate_type.call(context, Some(global_wasm), &[bytes.to_jsvalue()]) {
            Ok(val) => {
                if val.to_bool(context) {
                    Ok(())
                } else {
                    Err(CompileError::Validate(
                        "Not a valid wasm binary".to_string(),
                    ))
                }
            }
            Err(e) => Err(CompileError::Validate(format!(
                "Error while validating: {}",
                e.to_string(context).unwrap()
            ))),
        }
    }

    pub(crate) fn instantiate(
        &self,
        store: &mut impl AsStoreMut,
        imports: &Imports,
    ) -> Result<VMInstance, RuntimeError> {
        // Ensure all imports come from the same store.
        if imports
            .into_iter()
            .any(|(_, import)| !import.is_from_store(store))
        {
            #[cfg(feature = "std")]
            {
                return Err(RuntimeError::user(Box::new(
                    InstantiationError::DifferentStores,
                )));
            }
            #[cfg(not(feature = "std"))]
            {
                return Err(RuntimeError::new(
                    "cannot mix imports from different stores",
                ));
            }
        }

        let store = store.as_store_mut();
        let context = store.jsc().context();

        let mut imports_object = JSObject::new(context);
        for import_type in self.imports() {
            let resolved_import = imports.get_export(import_type.module(), import_type.name());
            if let Some(import) = resolved_import {
                let val = imports_object.get_property(context, import_type.module().to_string());
                if !val.is_undefined(context) {
                    // If the namespace is already set
                    let mut obj_val = val.to_object(context).unwrap();
                    obj_val.set_property(
                        context,
                        import_type.name().to_string(),
                        import.as_jsc_value(&store.as_store_ref()),
                    );
                } else {
                    // If the namespace doesn't exist
                    let mut import_namespace = JSObject::new(context);
                    import_namespace.set_property(
                        context,
                        import_type.name().to_string(),
                        import.as_jsc_value(&store.as_store_ref()),
                    );
                    imports_object
                        .set_property(
                            context,
                            import_type.module().to_string(),
                            import_namespace.to_jsvalue(),
                        )
                        .unwrap();
                }
            } else {
                warn!(
                    "import not found {}:{}",
                    import_type.module(),
                    import_type.name()
                );
            }
            // in case the import is not found, the JS Wasm VM will handle
            // the error for us, so we don't need to handle it
        }

        let instance_type = store.jsc().wasm_instance_type();
        instance_type
            .construct(
                context,
                &[self.module.to_jsvalue(), imports_object.to_jsvalue()],
            )
            .map_err(|e: JSValue| -> RuntimeError { e.into() })
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
    }

    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        self.raw_bytes.clone().ok_or(SerializeError::Generic(
            "Not able to serialize module".to_string(),
        ))
    }

    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        unsafe { Self::deserialize(engine, bytes) }
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        Self::from_binary(engine, &bytes.into_bytes()).map_err(DeserializeError::Compiler)
    }

    pub unsafe fn deserialize_from_file_unchecked(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        unsafe { Self::deserialize_unchecked(engine, bytes) }
    }

    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        unsafe { Self::deserialize(engine, bytes) }
    }

    pub fn set_name(&mut self, name: &str) -> bool {
        self.name = Some(name.to_string());
        true
    }

    pub fn imports<'a>(&'a self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + 'a>> {
        self.info().imports()
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        self.info().exports()
    }

    pub fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        self.info().custom_sections(name)
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        &self.info
    }
}

impl crate::Module {
    /// Consume [`self`] into a reference [`crate::backend::jsc::module::Module`].
    pub fn into_jsc(self) -> crate::backend::jsc::module::Module {
        match self.0 {
            BackendModule::Jsc(s) => s,
            _ => panic!("Not a `jsc` module!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::module::Module`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::module::Module {
        match self.0 {
            BackendModule::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` module!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::module::Module`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::module::Module {
        match self.0 {
            BackendModule::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` module!"),
        }
    }
}
