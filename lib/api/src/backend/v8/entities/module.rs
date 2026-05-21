//! Data types, functions and traits for `v8` runtime's `Module` implementation.
use std::{path::Path, sync::Arc};

use crate::{
    AsEngineRef, BackendModule, IntoBytes, Store, backend::v8::bindings::*,
    v8::utils::convert::IntoWasmerExternType,
};

use bytes::Bytes;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ExternType, FunctionType,
    GlobalType, ImportType, ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages,
    SerializeError, TableType, Type,
};
use wasmparser::{Parser, Payload};

#[derive(Debug)]
pub(crate) struct ModuleHandle {
    pub(crate) v8_shared_module_handle: *mut wasm_shared_module_t,
    pub(crate) orig_store: Store,
}

impl PartialEq for ModuleHandle {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            wasm_module_same(
                wasm_module_obtain(self.orig_store.as_v8().inner, self.v8_shared_module_handle),
                wasm_module_obtain(
                    other.orig_store.as_v8().inner,
                    other.v8_shared_module_handle,
                ),
            )
        }
    }
}

impl Eq for ModuleHandle {}

impl ModuleHandle {
    fn new(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let bytes = wasm_byte_vec_t {
            size: binary.len(),
            data: binary.as_ptr() as _,
        };

        let engine = engine.as_engine_ref().engine().clone();
        let store = Store::new(engine.clone());
        let engine = engine.as_v8().inner.engine;

        let inner = unsafe { wasm_module_new(store.as_v8().inner, &bytes as *const _) };

        if inner.is_null() {
            return Err(CompileError::Validate(
                "Failed to create V8 module: null module reference returned from V8".to_string(),
            ));
        }

        let inner = unsafe { wasm_module_share(inner) };

        if inner.is_null() {
            return Err(CompileError::Validate(
                "Failed to create V8 module: null module reference returned from V8".to_string(),
            ));
        }

        Ok(Self {
            v8_shared_module_handle: inner,
            orig_store: store,
        })
    }

    fn deserialize(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let bytes = wasm_byte_vec_t {
            size: binary.len(),
            data: binary.as_ptr() as _,
        };

        let engine = engine.as_engine_ref().engine().clone();
        let store = Store::new(engine.clone());
        let inner = unsafe { wasm_module_deserialize(store.as_v8().inner, &bytes as *const _) };
        if inner.is_null() {
            return Err(CompileError::Validate(
                "Failed to deserialize V8 module: null module reference returned from V8"
                    .to_string(),
            ));
        }

        let inner = unsafe { wasm_module_share(inner) };

        if inner.is_null() {
            return Err(CompileError::Validate(
                "Failed to create V8 module: null module reference returned from V8".to_string(),
            ));
        }

        Ok(Self {
            v8_shared_module_handle: inner,
            orig_store: store,
        })
    }

    #[tracing::instrument(skip(self))]
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let handle = unsafe {
            wasm_module_obtain(
                self.orig_store.as_v8().inner,
                self.v8_shared_module_handle as *const _,
            )
        };

        let mut bytes = wasm_byte_vec_t {
            size: 0,
            data: std::ptr::null_mut(),
        };

        let bytes = unsafe {
            wasm_module_serialize(handle, &mut bytes as *mut _);
            if bytes.data.is_null() || bytes.size == 0 {
                return Err(SerializeError::Generic(String::from(
                    "V8 returned an empty vector as serialized module",
                )));
            }
            std::slice::from_raw_parts(bytes.data as *mut u8, bytes.size)
        };

        Ok(bytes.to_vec())
    }
}

impl Drop for ModuleHandle {
    fn drop(&mut self) {
        unsafe { wasm_shared_module_delete(self.v8_shared_module_handle) }
    }
}

const DYLINK_SECTION_NAME: &str = "dylink.0";

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `module` in the `v8` runtime.
pub struct Module {
    pub(crate) handle: Arc<ModuleHandle>,
    name: Option<String>,
    imports: Vec<ImportType>,
    exports: Vec<ExportType>,
    // Copy of the section data needed by the dynamic linker, since the current
    // API cannot retrieve it later from the handle.
    dylink_section_data: Option<Vec<u8>>,
}

unsafe impl Send for Module {}
unsafe impl Sync for Module {}

impl Module {
    #[tracing::instrument(skip(engine, binary))]
    pub(crate) fn from_binary(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        tracing::info!("Creating module from binary");
        unsafe { Self::from_binary_unchecked(engine, binary) }
    }

    #[allow(clippy::arc_with_non_send_sync)]
    #[tracing::instrument(skip(engine, binary))]
    pub(crate) unsafe fn from_binary_unchecked(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        tracing::info!("Creating module from binary unchecked");
        let mut binary = binary.to_vec();
        let binary = binary.into_bytes();
        let module = ModuleHandle::new(engine, &binary)?;
        let info = crate::utils::polyfill::translate_module(&binary[..])
            .unwrap()
            .info;
        let imports = info.imports().collect();
        let exports = info.exports().collect();
        let dylink_section_data = get_dylink_section_data(&binary)?;

        Ok(Self {
            handle: Arc::new(module),
            name: info.name,
            imports,
            exports,
            dylink_section_data,
        })
    }

    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        let engine = engine.as_engine_ref().engine().clone();
        let store = super::store::Store::new(engine);
        let bytes = wasm_byte_vec_t {
            size: binary.len(),
            data: binary.as_ptr() as _,
        };
        let store = store.inner;
        unsafe {
            if !wasm_module_validate(store, &bytes as *const _) {
                return Err(CompileError::Validate(String::from(
                    "V8 could not validate the given module",
                )));
            }
        }

        Ok(())
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
    }

    #[tracing::instrument(skip(self))]
    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        let mut raw_bytes = self
            .name
            .clone()
            .unwrap_or_default()
            .bytes()
            .collect::<Vec<u8>>();
        let raw_bytes_off = raw_bytes.len();
        let mut v8_module_bytes = self.handle.serialize()?;

        let mut data = raw_bytes_off.to_ne_bytes().to_vec();

        data.append(&mut raw_bytes);
        data.append(&mut v8_module_bytes);

        Ok(data.into())
    }

    #[allow(clippy::arc_with_non_send_sync)]
    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        tracing::info!("Creating module from deserialize_unchecked");
        let binary = bytes.into_bytes();
        let off = &binary[0..8];
        let off = usize::from_ne_bytes(off.try_into().unwrap());
        let name_bytes = &binary[8..(8 + off)];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        let mod_bytes = &binary[(8 + off)..];
        let module = ModuleHandle::deserialize(engine, mod_bytes)?;
        let store = module.orig_store.as_v8().inner;
        let shared_handle = module.v8_shared_module_handle;
        let imports = v8_imports(store, shared_handle);
        let exports = v8_exports(store, shared_handle);
        let dylink_section_data = get_dylink_section_data(mod_bytes)?;

        Ok(Self {
            handle: Arc::new(module),
            name: Some(name),
            imports,
            exports,
            dylink_section_data,
        })
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        unsafe { Self::deserialize_unchecked(engine, bytes) }
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
        let imports = self.imports.clone();
        let len = imports.len();
        wasmer_types::ImportsIterator::new(Box::new(imports.into_iter()), len)
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        let exports = self.exports.clone();
        let len = exports.len();
        wasmer_types::ExportsIterator::new(Box::new(exports.into_iter()), len)
    }

    pub fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        if name == DYLINK_SECTION_NAME {
            Box::new(
                self.dylink_section_data
                    .iter()
                    .map(|data| data.clone().into_boxed_slice()),
            )
        } else {
            Box::new(std::iter::empty())
        }
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        panic!("no info for V8 modules")
    }
}

fn get_dylink_section_data(binary: &[u8]) -> Result<Option<Vec<u8>>, CompileError> {
    for payload in Parser::new(0).parse_all(binary) {
        if let Payload::CustomSection(section) = payload.map_err(|err| {
            CompileError::Validate(format!("Failed to parse custom sections: {err}"))
        })? && section.name() == DYLINK_SECTION_NAME
        {
            return Ok(Some(section.data().to_vec()));
        }
    }
    Ok(None)
}

fn v8_imports(
    store: *mut wasm_store_t,
    shared_handle: *mut wasm_shared_module_t,
) -> Vec<ImportType> {
    let mut imports = wasm_importtype_vec_t {
        size: 0,
        data: std::ptr::null_mut(),
    };

    unsafe {
        let module = wasm_module_obtain(store, shared_handle);
        if module.is_null() {
            panic!("Could not get imports: underlying module is null!");
        }

        wasm_module_imports(module as *const _, &mut imports as *mut _);

        let imports = if imports.data.is_null() || !imports.data.is_aligned() || imports.size == 0
        {
            vec![]
        } else {
            std::slice::from_raw_parts(imports.data, imports.size).to_vec()
        };

        imports
            .into_iter()
            .map(|i| {
                if i.is_null() {
                    panic!("null import returned from V8!");
                }

                let name = wasm_importtype_name(i as *const _);
                let name = std::slice::from_raw_parts((*name).data as *const u8, (*name).size);
                let name_str = String::from_utf8_lossy(name).to_string();
                let module = wasm_importtype_module(i as *const _);
                let module_str = if module.is_null()
                    || (*module).data.is_null()
                    || !(*module).data.is_aligned()
                    || (*module).size == 0
                {
                    String::new()
                } else {
                    let str =
                        std::slice::from_raw_parts((*module).data as *const u8, (*module).size);
                    String::from_utf8_lossy(str).to_string()
                };

                let ty = IntoWasmerExternType::into_wextt(wasm_importtype_type(i as *const _))
                    .unwrap_or_else(|err| panic!("{err}"));
                ImportType::new(&module_str, &name_str, ty)
            })
            .collect()
    }
}

fn v8_exports(
    store: *mut wasm_store_t,
    shared_handle: *mut wasm_shared_module_t,
) -> Vec<ExportType> {
    let mut exports = wasm_exporttype_vec_t {
        size: 0,
        data: std::ptr::null_mut(),
    };

    unsafe {
        let module = wasm_module_obtain(store, shared_handle);
        if module.is_null() {
            panic!("Could not get exports: underlying module is null!");
        }

        wasm_module_exports(module as *const _, &mut exports as *mut _);

        let exports = if exports.data.is_null() || !exports.data.is_aligned() || exports.size == 0
        {
            vec![]
        } else {
            std::slice::from_raw_parts(exports.data, exports.size).to_vec()
        };

        exports
            .into_iter()
            .map(|e| {
                if e.is_null() {
                    panic!("null export returned from V8!");
                }

                let name = wasm_exporttype_name(e as *const _);
                let name = std::slice::from_raw_parts((*name).data as *const u8, (*name).size);
                let name_str = String::from_utf8_lossy(name).to_string();
                let ty = IntoWasmerExternType::into_wextt(wasm_exporttype_type(e as *const _))
                    .unwrap_or_else(|err| panic!("{err}"));
                ExportType::new(&name_str, ty)
            })
            .collect()
    }
}

impl crate::Module {
    /// Consume [`self`] into a reference [`crate::backend::v8::module::Module`].
    pub fn into_v8(self) -> crate::backend::v8::module::Module {
        match self.0 {
            BackendModule::V8(s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::v8::module::Module`].
    pub fn as_v8(&self) -> &crate::backend::v8::module::Module {
        match self.0 {
            BackendModule::V8(ref s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::v8::module::Module`].
    pub fn as_v8_mut(&mut self) -> &mut crate::backend::v8::module::Module {
        match self.0 {
            BackendModule::V8(ref mut s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }
}
