use crate::errors::InstantiationError;
use crate::errors::RuntimeError;
use crate::imports::Imports;
use crate::jsc::as_js::AsJs;
use crate::store::AsStoreMut;
use crate::store::AsStoreRef;
use crate::vm::VMInstance;
use crate::Extern;
use crate::IntoBytes;
use crate::{AsEngineRef, ExportType, ImportType};
use bytes::Bytes;
use rusty_jsc::{JSObject, JSString, JSValue};
use std::path::Path;
#[cfg(feature = "tracing")]
use tracing::{debug, warn};
use wasmer_types::{
    CompileError, DeserializeError, ExportsIterator, ExternType, FunctionType, GlobalType,
    ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages, SerializeError, TableType, Type,
};

// /// JavascriptCore doesn't yet output the descriptor/types
// /// corresponding to each extern (import and export).
// ///
// /// This should be fixed once the JS-Types Wasm proposal is adopted
// /// by the browsers:
// /// https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md
// ///
// /// Until that happens, we annotate the module with the expected
// /// types so we can built on top of them at runtime.
// #[derive(Clone, PartialEq, Eq)]
// pub struct ModuleTypeHints {
//     /// The type hints for the imported types
//     pub imports: Vec<ExternType>,
//     /// The type hints for the exported types
//     pub exports: Vec<ExternType>,
// }

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
        let context = engine.engine().0.context();
        let bytes = JSObject::create_typed_array_with_bytes(&context, &mut binary).unwrap();
        let module_type = engine.engine().0.wasm_module_type();
        let module = module_type
            .construct(&context, &[bytes.to_jsvalue()])
            .map_err(|e| CompileError::Validate(format!("{}", e.to_string(&context))))?;

        Ok(Self::from_js_module(module, binary))
    }

    /// Creates a new WebAssembly module skipping any kind of validation from a javascript module
    ///
    pub(crate) unsafe fn from_js_module(module: JSObject, binary: impl IntoBytes) -> Self {
        let binary = binary.into_bytes();
        // The module is now validated, so we can safely parse it's types
        let info = crate::jsc::module_info_polyfill::translate_module(&binary[..])
            .unwrap()
            .info;

        // #[cfg(feature = "wasm-types-polyfill")]
        // let type_hints = ModuleTypeHints {
        //     imports: info
        //         .imports()
        //         .map(|import| import.ty().clone())
        //         .collect::<Vec<_>>(),
        //     exports: info
        //         .exports()
        //         .map(|export| export.ty().clone())
        //         .collect::<Vec<_>>(),
        // };

        Self {
            module,
            name: info.name.clone(),
            raw_bytes: Some(binary.into_bytes()),
            info,
        }
    }

    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        let engine = engine.as_engine_ref();
        let context = engine.engine().0.context();
        let mut binary = binary.to_vec();
        let bytes = JSObject::create_typed_array_with_bytes(&context, &mut binary).unwrap();

        let global_wasm = engine.engine().0.global_wasm();
        let validate_type = engine.engine().0.wasm_validate_type();

        match validate_type.call(&context, global_wasm.clone(), &[bytes.to_jsvalue()]) {
            Ok(val) => {
                if val.to_bool(&context) {
                    Ok(())
                } else {
                    Err(CompileError::Validate(format!("Not a valid wasm binary")))
                }
            }
            Err(e) => Err(CompileError::Validate(format!(
                "Error while validating: {}",
                e.to_string(&context)
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
            return Err(RuntimeError::user(Box::new(
                InstantiationError::DifferentStores,
            )));
        }

        let store = store.as_store_mut();
        let context = store.engine().0.context();

        let mut imports_object = JSObject::new(&context);
        for import_type in self.imports() {
            let resolved_import = imports.get_export(import_type.module(), import_type.name());
            if let Some(import) = resolved_import {
                let val = imports_object.get_property(&context, import_type.module().into());
                if !val.is_undefined(&context) {
                    // If the namespace is already set
                    let mut obj_val = val.to_object(&context);
                    obj_val.set_property(
                        &context,
                        import_type.name().into(),
                        import.as_jsvalue(&store.as_store_ref()),
                    );
                } else {
                    // If the namespace doesn't exist
                    let mut import_namespace = JSObject::new(&context);
                    import_namespace.set_property(
                        &context,
                        import_type.name().into(),
                        import.as_jsvalue(&store.as_store_ref()),
                    );
                    imports_object
                        .set_property(
                            &context,
                            import_type.module().into(),
                            import_namespace.to_jsvalue(),
                        )
                        .unwrap();
                }
            } else {
                #[cfg(feature = "tracing")]
                warn!(
                    "import not found {}:{}",
                    import_type.module(),
                    import_type.name()
                );
            }
            // in case the import is not found, the JS Wasm VM will handle
            // the error for us, so we don't need to handle it
        }

        let instance_type = store.engine().0.wasm_instance_type();
        let instance = instance_type.construct(
            &context,
            &[self.module.to_jsvalue(), imports_object.to_jsvalue()],
        );
        Ok(instance.map_err(|e: JSValue| -> RuntimeError { e.into() })?)
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
    }

    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        return self.raw_bytes.clone().ok_or(SerializeError::Generic(
            "Not able to serialize module".to_string(),
        ));
    }

    pub unsafe fn deserialize(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        return Self::from_binary(_engine, &_bytes.into_bytes())
            .map_err(|e| DeserializeError::Compiler(e));
    }

    pub fn deserialize_checked(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        unimplemented!();
    }

    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        Self::deserialize(engine, bytes)
    }

    pub fn deserialize_from_file_checked(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        Self::deserialize_checked(engine, bytes)
    }

    pub fn set_name(&mut self, name: &str) -> bool {
        self.name = Some(name.to_string());
        true
    }

    pub fn imports<'a>(
        &'a self,
        // engine: &impl AsEngineRef,
    ) -> ImportsIterator<impl Iterator<Item = ImportType> + 'a> {
        self.info().imports()
        // let imports = self.type_hints.as_ref().unwrap().imports;
        // ImportsIterator::new(imports.iter(), imports.len())
        // let engine = engine.as_engine_ref();
        // let context = engine.engine().0.context();
        // let module_type = engine.engine().0.wasm_module_type();
        // let custom_sections_func = module_type.get_property(&context, "imports".to_string());
        // let imports = custom_sections_func
        //     .to_object(&context)
        //     .call(
        //         &context,
        //         module_type.clone(),
        //         &[self.module.clone().to_jsvalue()],
        //     )
        //     .unwrap()
        //     .to_object(&context);
        // let length = imports
        //     .get_property(&context, "length".to_string())
        //     .to_number(&context) as u32;
        // let iter = (0..length)
        //     .map(|i| {
        //         let val = imports
        //             .get_property_at_index(&context, i)
        //             .unwrap()
        //             .to_object(&context);
        //         // Annotation is here to prevent spurious IDE warnings.
        //         #[allow(unused_unsafe)]
        //         unsafe {
        //             let module = val
        //                 .get_property(&context, "module".to_string())
        //                 .to_string(&context);
        //             let field = val
        //                 .get_property(&context, "name".to_string())
        //                 .to_string(&context);
        //             let kind = val
        //                 .get_property(&context, "kind".to_string())
        //                 .to_string(&context);
        //             let type_hint: Option<ExternType> = self
        //                 .type_hints
        //                 .as_ref()
        //                 .map(|hints| hints.imports[i as usize].clone());
        //             let extern_type = if let Some(hint) = type_hint {
        //                 hint
        //             } else {
        //                 match kind.as_str() {
        //                     "function" => {
        //                         let func_type = FunctionType::new(vec![], vec![]);
        //                         ExternType::Function(func_type)
        //                     }
        //                     "global" => {
        //                         let global_type = GlobalType::new(Type::I32, Mutability::Const);
        //                         ExternType::Global(global_type)
        //                     }
        //                     "memory" => {
        //                         // The javascript API does not yet expose these properties so without
        //                         // the type_hints we don't know what memory to import.
        //                         let memory_type = MemoryType::new(Pages(1), None, false);
        //                         ExternType::Memory(memory_type)
        //                     }
        //                     "table" => {
        //                         let table_type = TableType::new(Type::FuncRef, 1, None);
        //                         ExternType::Table(table_type)
        //                     }
        //                     _ => unimplemented!(),
        //                 }
        //             };
        //             ImportType::new(&module, &field, extern_type)
        //         }
        //     })
        //     .collect::<Vec<_>>()
        //     .into_iter();
        // ImportsIterator::new(iter, length as usize)
    }

    // /// Set the type hints for this module.
    // ///
    // /// Returns an error if the hints doesn't match the shape of
    // /// import or export types of the module.
    // #[allow(unused)]
    // pub fn set_type_hints(&mut self, type_hints: ModuleTypeHints) -> Result<(), String> {
    //     // let exports = WebAssembly::Module::exports(&self.module);
    //     // // Check exports
    //     // if exports.length() as usize != type_hints.exports.len() {
    //     //     return Err("The exports length must match the type hints lenght".to_owned());
    //     // }
    //     // for (i, val) in exports.iter().enumerate() {
    //     //     // Annotation is here to prevent spurious IDE warnings.
    //     //     #[allow(unused_unsafe)]
    //     //     let kind = unsafe {
    //     //         Reflect::get(val.as_ref(), &"kind".into())
    //     //             .unwrap()
    //     //             .as_string()
    //     //             .unwrap()
    //     //     };
    //     //     // It is safe to unwrap as we have already checked for the exports length
    //     //     let type_hint = type_hints.exports.get(i).unwrap();
    //     //     let expected_kind = match type_hint {
    //     //         ExternType::Function(_) => "function",
    //     //         ExternType::Global(_) => "global",
    //     //         ExternType::Memory(_) => "memory",
    //     //         ExternType::Table(_) => "table",
    //     //     };
    //     //     if expected_kind != kind.as_str() {
    //     //         return Err(format!("The provided type hint for the export {} is {} which doesn't match the expected kind: {}", i, kind.as_str(), expected_kind));
    //     //     }
    //     // }
    //     self.type_hints = Some(type_hints);
    //     Ok(())
    // }

    pub fn exports<'a>(
        &'a self,
        // engine: &impl AsEngineRef,
    ) -> ExportsIterator<impl Iterator<Item = ExportType> + 'a> {
        self.info().exports()
        // let exports = self.type_hints.as_ref().unwrap().exports;
        // ExportsIterator::new(exports.iter(), exports.len())
        // let engine = engine.as_engine_ref();
        // let context = engine.engine().0.context();
        // let module_type = engine.engine().0.wasm_module_type();
        // let custom_sections_func = module_type.get_property(&context, "exports".to_string());
        // let exports = custom_sections_func
        //     .to_object(&context)
        //     .call(
        //         &context,
        //         module_type.clone(),
        //         &[self.module.clone().to_jsvalue()],
        //     )
        //     .unwrap()
        //     .to_object(&context);
        // let length = exports
        //     .get_property(&context, "length".to_string())
        //     .to_number(&context) as u32;
        // let iter = (0..length)
        //     .map(|i| {
        //         let val = exports
        //             .get_property_at_index(&context, i)
        //             .unwrap()
        //             .to_object(&context);
        //         // Annotation is here to prevent spurious IDE warnings.
        //         #[allow(unused_unsafe)]
        //         let field = unsafe {
        //             val.get_property(&context, "name".to_string())
        //                 .to_string(&context)
        //         };
        //         // Annotation is here to prevent spurious IDE warnings.
        //         #[allow(unused_unsafe)]
        //         let kind = unsafe {
        //             val.get_property(&context, "kind".to_string())
        //                 .to_string(&context)
        //         };
        //         let type_hint: Option<ExternType> = self
        //             .type_hints
        //             .as_ref()
        //             .map(|hints| hints.exports[i as usize].clone());
        //         let extern_type = if let Some(hint) = type_hint {
        //             hint
        //         } else {
        //             // The default types
        //             match kind.as_str() {
        //                 "function" => {
        //                     let func_type = FunctionType::new(vec![], vec![]);
        //                     ExternType::Function(func_type)
        //                 }
        //                 "global" => {
        //                     let global_type = GlobalType::new(Type::I32, Mutability::Const);
        //                     ExternType::Global(global_type)
        //                 }
        //                 "memory" => {
        //                     let memory_type = MemoryType::new(Pages(1), None, false);
        //                     ExternType::Memory(memory_type)
        //                 }
        //                 "table" => {
        //                     let table_type = TableType::new(Type::FuncRef, 1, None);
        //                     ExternType::Table(table_type)
        //                 }
        //                 _ => unimplemented!(),
        //             }
        //         };
        //         ExportType::new(&field, extern_type)
        //     })
        //     .collect::<Vec<_>>()
        //     .into_iter();
        // ExportsIterator::new(iter, length as usize)
    }

    pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Box<[u8]>> + 'a {
        self.info().custom_sections(name)
        // let engine = engine.as_engine_ref();
        // let context = engine.engine().0.context();
        // let module_type = engine.engine().0.wasm_module_type();
        // let custom_sections_func = module_type.get_property(&context, "customSections".to_string());
        // let name = JSString::from_utf8(name.to_string())
        //     .unwrap()
        //     .to_jsvalue(&context);
        // let results = custom_sections_func
        //     .to_object(&context)
        //     .call(
        //         &context,
        //         module_type.clone(),
        //         &[self.module.clone().to_jsvalue(), name],
        //     )
        //     .unwrap()
        //     .to_object(&context);
        // let length = results
        //     .get_property(&context, "length".to_string())
        //     .to_number(&context) as u32;

        // let array_buffers = (0..length).map(|i| {
        //     let array_buffer = results.get_property_at_index(&context, i).unwrap();
        //     array_buffer
        //         .to_object(&context)
        //         .get_array_buffer(&context)
        //         .unwrap()
        //         .to_owned()
        //         .into_boxed_slice()
        // });
        // array_buffers.collect::<Vec<Box<[u8]>>>().into_iter()
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        &self.info
    }
}

// impl From<WebAssembly::Module> for Module {
//     fn from(module: WebAssembly::Module) -> Module {
//         Module {
//             module,
//             name: None,
//             type_hints: None,
//             #[cfg(feature = "js-serializable-module")]
//             raw_bytes: None,
//         }
//     }
// }

// impl<T: IntoBytes> From<(WebAssembly::Module, T)> for crate::module::Module {
//     fn from((module, binary): (WebAssembly::Module, T)) -> crate::module::Module {
//         unsafe { crate::module::Module(Module::from_js_module(module, binary.into_bytes())) }
//     }
// }

// impl From<WebAssembly::Module> for crate::module::Module {
//     fn from(module: WebAssembly::Module) -> crate::module::Module {
//         crate::module::Module(module.into())
//     }
// }
