use std::{collections::HashMap, path::Path};

use bytes::Bytes;
use js_sys::{Reflect, Uint8Array, WebAssembly};
use tracing::{debug, warn};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use wasm_bindgen::{JsValue, prelude::*};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;
use wasmer_types::{
    CompileError, DeserializeError, ExportIndex, ExportType, ExportsIterator, ExternType,
    FunctionType, GlobalIndex, GlobalType, ImportIndex, ImportType, ImportsIterator, InitExpr,
    InitExprOp, MemoryType, ModuleInfo, Mutability, Pages, SerializeError, TableIndex, TableType,
    Type,
};

use crate::{
    AsEngineRef, AsStoreMut, BackendModule, Extern, Imports, InstantiationError, IntoBytes,
    RuntimeError,
    js::{
        utils::{convert::AsJs as _, shared_handle::SharedJsHandle},
        vm::VMInstance,
    },
};

/// WebAssembly in the browser doesn't yet output the descriptor/types
/// corresponding to each extern (import and export).
///
/// This should be fixed once the JS-Types Wasm proposal is adopted
/// by the browsers:
/// <https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md>
///
/// Until that happens, we annotate the module with the expected
/// types so we can built on top of them at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleTypeHints {
    /// The type hints for the imported types
    pub imports: Vec<ExternType>,
    /// The type hints for the exported types
    pub exports: Vec<ExternType>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Module {
    module: SharedJsHandle,
    name: Option<String>,
    #[cfg(feature = "wasm-types-polyfill")]
    info: ModuleInfo,
    // WebAssembly type hints
    type_hints: Option<ModuleTypeHints>,
    #[cfg(feature = "js-serializable-module")]
    raw_bytes: Option<Bytes>,
}

#[cfg(feature = "wasm-types-polyfill")]
fn evaluate_i32_init_expr(
    expression: &InitExpr,
    globals: &HashMap<GlobalIndex, i64>,
) -> Option<u32> {
    let mut stack = Vec::<i64>::new();
    for operation in expression.ops() {
        match operation {
            InitExprOp::GlobalGetI32(index) | InitExprOp::GlobalGetI64(index) => {
                stack.push(*globals.get(index)?);
            }
            InitExprOp::I32Const(value) => stack.push(i64::from(*value)),
            InitExprOp::I64Const(value) => stack.push(*value),
            InitExprOp::I32Add | InitExprOp::I64Add => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(lhs.checked_add(rhs)?);
            }
            InitExprOp::I32Sub | InitExprOp::I64Sub => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(lhs.checked_sub(rhs)?);
            }
            InitExprOp::I32Mul | InitExprOp::I64Mul => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(lhs.checked_mul(rhs)?);
            }
        }
    }
    u32::try_from(stack.pop()?)
        .ok()
        .filter(|_| stack.is_empty())
}

impl From<Module> for JsValue {
    fn from(val: Module) -> Self {
        Self::from(val.local_module())
    }
}

impl Module {
    fn local_module(&self) -> WebAssembly::Module {
        if let Some(module) = self.module.get() {
            return module;
        }
        // A running sibling can observe dlopen before receiving postMessage.
        #[cfg(feature = "js-serializable-module")]
        if let Some(bytes) = &self.raw_bytes {
            let bytes = Uint8Array::from(bytes.as_ref());
            let module = WebAssembly::Module::new(&bytes.into())
                .expect("failed to compile a shared module in this worker");
            self.module.install(module.clone());
            return module;
        }
        panic!("module was not transported to this worker and has no retained bytes");
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn new_async(
        _engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        // Copy the bytes into JavaScript-owned memory before awaiting. A view
        // into Wasm linear memory could be invalidated if that memory grows.
        let js_bytes = Uint8Array::from(binary);
        let module = JsFuture::from(WebAssembly::compile(&js_bytes))
            .await
            .map_err(|error| {
                CompileError::Validate(
                    error
                        .as_string()
                        .or_else(|| {
                            Reflect::get(&error, &JsValue::from_str("message"))
                                .ok()
                                .and_then(|message| message.as_string())
                        })
                        .unwrap_or_else(|| "Unknown validation error".to_owned()),
                )
            })?
            .dyn_into::<WebAssembly::Module>()
            .map_err(|_| {
                CompileError::Validate(
                    "WebAssembly.compile returned an unexpected value".to_owned(),
                )
            })?;

        Ok(unsafe { Self::from_js_module(module, binary) })
    }

    pub(crate) fn from_binary(
        _engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        unsafe { Self::from_binary_unchecked(_engine, binary) }
    }

    pub(crate) unsafe fn from_binary_unchecked(
        _engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        let js_bytes = unsafe { Uint8Array::view(binary) };
        let module = WebAssembly::Module::new(&js_bytes.into()).map_err(|e| {
            CompileError::Validate(
                e.as_string()
                    .unwrap_or("Unknown validation error".to_string())
                    .to_string(),
            )
        })?;
        Ok(unsafe { Self::from_js_module(module, binary) })
    }

    /// Creates a new WebAssembly module skipping any kind of validation from a javascript module
    pub(crate) unsafe fn from_js_module(
        module: WebAssembly::Module,
        binary: impl IntoBytes,
    ) -> Self {
        let binary = binary.into_bytes();

        // The module is now validated, so we can safely parse it's types
        #[cfg(feature = "wasm-types-polyfill")]
        let (type_hints, name, module_info) = {
            let translated = crate::polyfill::translate_module(&binary[..]).unwrap();

            (
                Some(ModuleTypeHints {
                    imports: translated
                        .info
                        .imports()
                        .map(|import| import.ty().clone())
                        .collect::<Vec<_>>(),
                    exports: translated
                        .info
                        .exports()
                        .map(|export| export.ty().clone())
                        .collect::<Vec<_>>(),
                }),
                translated.info.name.clone(),
                translated.info,
            )
        };
        #[cfg(not(feature = "wasm-types-polyfill"))]
        let (type_hints, name) = (None, None);

        Self {
            module: SharedJsHandle::new(module),
            type_hints,
            name,
            #[cfg(feature = "wasm-types-polyfill")]
            info: module_info,
            #[cfg(feature = "js-serializable-module")]
            raw_bytes: Some(binary),
        }
    }

    pub fn validate(_engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        let js_bytes = unsafe { Uint8Array::view(binary) };
        // Annotation is here to prevent spurious IDE warnings.
        #[allow(unused_unsafe)]
        unsafe {
            match WebAssembly::validate(&js_bytes.into()) {
                Ok(true) => Ok(()),
                _ => Err(CompileError::Validate("Invalid Wasm file".to_owned())),
            }
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

        let imports_object = js_sys::Object::new();
        let mut import_externs: Vec<Extern> = vec![];
        for import_type in self.imports() {
            let resolved_import = imports.get_export(import_type.module(), import_type.name());
            // Annotation is here to prevent spurious IDE warnings.
            #[allow(unused_variables)]
            if let wasmer_types::ExternType::Memory(mem_ty) = import_type.ty() {
                if resolved_import.is_some() {
                    debug!("imported shared memory {:?}", &mem_ty);
                } else {
                    warn!(
                        "Error while importing {0:?}.{1:?}: memory. Expected {2:?}",
                        import_type.module(),
                        import_type.name(),
                        import_type.ty(),
                    );
                }
            }
            // Annotation is here to prevent spurious IDE warnings.
            #[allow(unused_unsafe)]
            unsafe {
                if let Some(import) = resolved_import {
                    let val = js_sys::Reflect::get(&imports_object, &import_type.module().into())?;
                    if !val.is_undefined() {
                        // If the namespace is already set
                        js_sys::Reflect::set(
                            &val,
                            &import_type.name().into(),
                            &import.as_jsvalue(&store.as_store_ref()),
                        )?;
                    } else {
                        // If the namespace doesn't exist
                        let import_namespace = js_sys::Object::new();
                        js_sys::Reflect::set(
                            &import_namespace,
                            &import_type.name().into(),
                            &import.as_jsvalue(&store.as_store_ref()),
                        )?;
                        js_sys::Reflect::set(
                            &imports_object,
                            &import_type.module().into(),
                            &import_namespace.into(),
                        )?;
                    }
                    import_externs.push(import);
                } else {
                    warn!(
                        "import not found {}:{}",
                        import_type.module(),
                        import_type.name()
                    );
                }
            }
            // in case the import is not found, the JS Wasm VM will handle
            // the error for us, so we don't need to handle it
        }
        let instance = WebAssembly::Instance::new(&self.local_module(), &imports_object)
            .map_err(|e: JsValue| -> RuntimeError { e.into() })?;
        #[cfg(feature = "wasm-types-polyfill")]
        self.annotate_table_functions(store, imports, &instance);
        Ok(instance)
    }

    #[cfg(feature = "wasm-types-polyfill")]
    fn annotate_table_functions(
        &self,
        store: &mut impl AsStoreMut,
        imports: &Imports,
        instance: &WebAssembly::Instance,
    ) {
        let mut tables = HashMap::<TableIndex, WebAssembly::Table>::new();
        let mut globals = HashMap::<GlobalIndex, i64>::new();

        for (key, import_index) in &self.info.imports {
            let Some(extern_) = imports.get_export(&key.module, &key.field) else {
                continue;
            };
            match (import_index, extern_) {
                (ImportIndex::Table(index), Extern::Table(table)) => {
                    tables.insert(*index, table.as_js().handle.table.clone());
                }
                (ImportIndex::Global(index), Extern::Global(global)) => {
                    let value = match global.get(store) {
                        crate::Value::I32(value) => i64::from(value),
                        crate::Value::I64(value) => value,
                        _ => continue,
                    };
                    globals.insert(*index, value);
                }
                _ => {}
            }
        }
        let instance_exports = instance.exports();
        for (name, export_index) in &self.info.exports {
            let ExportIndex::Table(index) = export_index else {
                continue;
            };
            let Ok(value) = Reflect::get(&instance_exports, &name.into()) else {
                continue;
            };
            tables.insert(*index, value.into());
        }
        for initializer in &self.info.table_initializers {
            let Some(table) = tables.get(&initializer.table_index) else {
                continue;
            };
            let Some(start) = evaluate_i32_init_expr(&initializer.offset_expr, &globals) else {
                continue;
            };
            for (offset, function_index) in initializer.elements.iter().enumerate() {
                let Some(signature_index) = self.info.functions.get(*function_index) else {
                    continue;
                };
                let Some(function_type) = self.info.signatures.get(*signature_index) else {
                    continue;
                };
                let Ok(function) = table.get(start.saturating_add(offset as u32)) else {
                    continue;
                };
                crate::js::vm::VMFunction::annotate_type(&function, function_type);
            }
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
    }

    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        #[cfg(feature = "js-serializable-module")]
        return self.raw_bytes.clone().ok_or(SerializeError::Generic(
            "Not able to serialize module".to_string(),
        ));

        #[cfg(not(feature = "js-serializable-module"))]
        return Err(SerializeError::Generic(
            "You need to enable the `js-serializable-module` feature flag to serialize a `Module`"
                .to_string(),
        ));
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub unsafe fn deserialize_unchecked(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        #[cfg(feature = "js-serializable-module")]
        return Self::from_binary(_engine, &_bytes.into_bytes())
            .map_err(|e| DeserializeError::Compiler(e));

        #[cfg(not(feature = "js-serializable-module"))]
        return Err(DeserializeError::Generic("You need to enable the `js-serializable-module` feature flag to deserialize a `Module`".to_string()));
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub unsafe fn deserialize(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        #[cfg(feature = "js-serializable-module")]
        return Self::from_binary(_engine, &_bytes.into_bytes())
            .map_err(|e| DeserializeError::Compiler(e));

        #[cfg(not(feature = "js-serializable-module"))]
        return Err(DeserializeError::Generic("You need to enable the `js-serializable-module` feature flag to deserialize a `Module`".to_string()));
    }

    pub unsafe fn deserialize_from_file_unchecked(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        unsafe { Self::deserialize(engine, bytes) }
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
        // match Reflect::set(self.module.as_ref(), &"wasmer_name".into(), &name.into()) {
        //     Ok(_) => true,
        //     _ => false
        // }
        // Arc::get_mut(&mut self.artifact)
        //     .and_then(|artifact| artifact.module_mut())
        //     .map(|mut module_info| {
        //         module_info.info.name = Some(name.to_string());
        //         true
        //     })
        //     .unwrap_or(false)
    }

    pub fn imports<'a>(&'a self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + 'a>> {
        let imports = WebAssembly::Module::imports(&self.local_module());
        let iter = imports
            .iter()
            .enumerate()
            .map(move |(i, val)| {
                // Annotation is here to prevent spurious IDE warnings.
                #[allow(unused_unsafe)]
                unsafe {
                    let module = Reflect::get(val.as_ref(), &"module".into())
                        .unwrap()
                        .as_string()
                        .unwrap();
                    let field = Reflect::get(val.as_ref(), &"name".into())
                        .unwrap()
                        .as_string()
                        .unwrap();
                    let kind = Reflect::get(val.as_ref(), &"kind".into())
                        .unwrap()
                        .as_string()
                        .unwrap();
                    let type_hint = self
                        .type_hints
                        .as_ref()
                        .map(|hints| hints.imports.get(i).unwrap().clone());
                    let extern_type = if let Some(hint) = type_hint {
                        hint
                    } else {
                        match kind.as_str() {
                            "function" => {
                                let func_type = FunctionType::new(vec![], vec![]);
                                ExternType::Function(func_type)
                            }
                            "global" => {
                                let global_type = GlobalType::new(Type::I32, Mutability::Const);
                                ExternType::Global(global_type)
                            }
                            "memory" => {
                                // The javascript API does not yet expose these properties so without
                                // the type_hints we don't know what memory to import.
                                let memory_type = MemoryType::new(Pages(1), None, false);
                                ExternType::Memory(memory_type)
                            }
                            "table" => {
                                let table_type = TableType::new(Type::FuncRef, 1, None);
                                ExternType::Table(table_type)
                            }
                            _ => unimplemented!(),
                        }
                    };
                    ImportType::new(&module, &field, extern_type)
                }
            })
            .collect::<Vec<_>>()
            .into_iter();
        ImportsIterator::new(Box::new(iter), imports.length() as usize)
    }

    /// Set the type hints for this module.
    ///
    /// Returns an error if the hints doesn't match the shape of
    /// import or export types of the module.
    #[allow(unused)]
    pub fn set_type_hints(&mut self, type_hints: ModuleTypeHints) -> Result<(), String> {
        let exports = WebAssembly::Module::exports(&self.local_module());
        // Check exports
        if exports.length() as usize != type_hints.exports.len() {
            return Err("The exports length must match the type hints length".to_owned());
        }
        for (i, val) in exports.iter().enumerate() {
            // Annotation is here to prevent spurious IDE warnings.
            #[allow(unused_unsafe)]
            let kind = unsafe {
                Reflect::get(val.as_ref(), &"kind".into())
                    .unwrap()
                    .as_string()
                    .unwrap()
            };
            // It is safe to unwrap as we have already checked for the exports length
            let type_hint = type_hints.exports.get(i).unwrap();
            let expected_kind = match type_hint {
                ExternType::Function(_) => "function",
                ExternType::Global(_) => "global",
                ExternType::Memory(_) => "memory",
                ExternType::Table(_) => "table",
                ExternType::Tag(_) => "tag",
            };
            if expected_kind != kind.as_str() {
                return Err(format!(
                    "The provided type hint for the export {} is {} which doesn't match the expected kind: {}",
                    i,
                    kind.as_str(),
                    expected_kind
                ));
            }
        }
        self.type_hints = Some(type_hints);
        Ok(())
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        let exports = WebAssembly::Module::exports(&self.local_module());
        let iter = exports
            .iter()
            .enumerate()
            .map(move |(i, val)| {
                // Annotation is here to prevent spurious IDE warnings.
                #[allow(unused_unsafe)]
                let field = unsafe {
                    Reflect::get(val.as_ref(), &"name".into())
                        .unwrap()
                        .as_string()
                        .unwrap()
                };
                // Annotation is here to prevent spurious IDE warnings.
                #[allow(unused_unsafe)]
                let kind = unsafe {
                    Reflect::get(val.as_ref(), &"kind".into())
                        .unwrap()
                        .as_string()
                        .unwrap()
                };
                let type_hint = self
                    .type_hints
                    .as_ref()
                    .map(|hints| hints.exports.get(i).unwrap().clone());
                let extern_type = if let Some(hint) = type_hint {
                    hint
                } else {
                    // The default types
                    match kind.as_str() {
                        "function" => {
                            let func_type = FunctionType::new(vec![], vec![]);
                            ExternType::Function(func_type)
                        }
                        "global" => {
                            let global_type = GlobalType::new(Type::I32, Mutability::Const);
                            ExternType::Global(global_type)
                        }
                        "memory" => {
                            let memory_type = MemoryType::new(Pages(1), None, false);
                            ExternType::Memory(memory_type)
                        }
                        "table" => {
                            let table_type = TableType::new(Type::FuncRef, 1, None);
                            ExternType::Table(table_type)
                        }
                        _ => unimplemented!(),
                    }
                };
                ExportType::new(&field, extern_type)
            })
            .collect::<Vec<_>>()
            .into_iter();
        ExportsIterator::new(Box::new(iter), exports.length() as usize)
    }

    pub fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        Box::new(
            WebAssembly::Module::custom_sections(&self.local_module(), name)
                .iter()
                .map(move |buf_val| {
                    let typebuf: js_sys::Uint8Array = js_sys::Uint8Array::new(&buf_val);
                    typebuf.to_vec().into_boxed_slice()
                })
                .collect::<Vec<Box<[u8]>>>()
                .into_iter(),
        )
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        #[cfg(feature = "wasm-types-polyfill")]
        {
            &self.info
        }
        #[cfg(not(feature = "wasm-types-polyfill"))]
        {
            unimplemented!("module info requires the wasm-types-polyfill feature")
        }
    }
}

impl From<WebAssembly::Module> for Module {
    #[track_caller]
    fn from(module: WebAssembly::Module) -> Self {
        Self {
            module: SharedJsHandle::new(module),
            name: None,
            type_hints: None,
            #[cfg(feature = "wasm-types-polyfill")]
            info: ModuleInfo::default(),
            #[cfg(feature = "js-serializable-module")]
            raw_bytes: None,
        }
    }
}

impl<T: IntoBytes> From<(WebAssembly::Module, T)> for crate::module::Module {
    fn from((module, binary): (WebAssembly::Module, T)) -> Self {
        unsafe {
            Self(BackendModule::Js(Module::from_js_module(
                module,
                binary.into_bytes(),
            )))
        }
    }
}

impl From<WebAssembly::Module> for crate::module::Module {
    fn from(module: WebAssembly::Module) -> Self {
        Self(BackendModule::Js(module.into()))
    }
}
impl From<crate::module::Module> for WebAssembly::Module {
    fn from(value: crate::module::Module) -> Self {
        value.into_js().local_module()
    }
}

impl crate::Module {
    /// Consume [`self`] into a reference [`crate::backend::js::module::Module`].
    pub fn into_js(self) -> crate::backend::js::module::Module {
        match self.0 {
            BackendModule::Js(s) => s,
            _ => panic!("Not a `js` module!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::module::Module`].
    pub fn as_js(&self) -> &crate::backend::js::module::Module {
        match &self.0 {
            BackendModule::Js(s) => s,
            _ => panic!("Not a `js` module!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::module::Module`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::module::Module {
        match &mut self.0 {
            BackendModule::Js(s) => s,
            _ => panic!("Not a `js` module!"),
        }
    }
}
