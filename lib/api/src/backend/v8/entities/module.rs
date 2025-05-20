//! Data types, functions and traits for `v8` runtime's `Module` implementation.
use std::{path::Path, sync::Arc};

use crate::{
    backend::v8::bindings::*, v8::utils::convert::IntoWasmerExternType, AsEngineRef, BackendModule,
    IntoBytes, Store,
};

use bytes::Bytes;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ExternType, FunctionType,
    GlobalType, ImportType, ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages,
    SerializeError, TableType, Type,
};

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
            return Err(CompileError::Validate(format!(
                "Failed to create V8 module: null module reference returned from V8"
            )));
        }

        let inner = unsafe { wasm_module_share(inner) };

        if inner.is_null() {
            return Err(CompileError::Validate(format!(
                "Failed to create V8 module: null module reference returned from V8"
            )));
        }

        Ok(ModuleHandle {
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
            return Err(CompileError::Validate(format!(
                "Failed to deserialize V8 module: null module reference returned from V8"
            )));
        }

        let inner = unsafe { wasm_module_share(inner) };

        if inner.is_null() {
            return Err(CompileError::Validate(format!(
                "Failed to create V8 module: null module reference returned from V8"
            )));
        }

        Ok(ModuleHandle {
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

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `module` in the `v8` runtime.
pub struct Module {
    pub(crate) handle: Arc<ModuleHandle>,
    name: Option<String>,
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

        Ok(Self {
            handle: Arc::new(module),
            name: info.name,
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

        Ok(Self {
            handle: Arc::new(module),
            name: Some(name),
        })
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        Self::deserialize_unchecked(engine, bytes)
    }

    pub unsafe fn deserialize_from_file_unchecked(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        Self::deserialize_unchecked(engine, bytes)
    }

    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let bytes = std::fs::read(path.as_ref())?;
        Self::deserialize(engine, bytes)
    }

    pub fn set_name(&mut self, name: &str) -> bool {
        self.name = Some(name.to_string());
        true
    }

    pub fn imports<'a>(&'a self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + 'a>> {
        let mut imports = wasm_importtype_vec_t {
            size: 0,
            data: std::ptr::null_mut(),
        };

        let store = self.handle.orig_store.as_v8().inner;
        let shared_handle = self.handle.v8_shared_module_handle;
        let imports = unsafe {
            let module = wasm_module_obtain(store, shared_handle);
            if module.is_null() {
                panic!("Could not get imports: underlying module is null!");
            }

            wasm_module_imports(module as *const _, &mut imports as *mut _);

            let imports =
                if imports.data.is_null() || !imports.data.is_aligned() || imports.size == 0 {
                    vec![]
                } else {
                    std::slice::from_raw_parts(imports.data, imports.size).to_vec()
                };
            let mut wasmer_imports = vec![];

            for i in imports.into_iter() {
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

                let ty = IntoWasmerExternType::into_wextt(wasm_importtype_type(i as *const _));
                if ty.is_err() {
                    panic!("{}", ty.unwrap_err());
                }

                let ty = ty.unwrap();
                wasmer_imports.push(ImportType::new(&module_str, &name_str, ty))
            }

            wasmer_imports
        };
        let len = imports.len();
        wasmer_types::ImportsIterator::new(Box::new(imports.into_iter()), len)
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        let mut exports = wasm_exporttype_vec_t {
            size: 0,
            data: std::ptr::null_mut(),
        };

        let store = self.handle.orig_store.as_v8().inner;
        let shared_handle = self.handle.v8_shared_module_handle;
        let exports = unsafe {
            let module = wasm_module_obtain(store, shared_handle);
            if module.is_null() {
                panic!("Could not get imports: underlying module is null!");
            }

            wasm_module_exports(module as *const _, &mut exports as *mut _);

            let exports = std::slice::from_raw_parts(exports.data, exports.size).to_vec();
            let mut wasmer_exports = vec![];

            for e in exports.into_iter() {
                if e.is_null() {
                    panic!("null import returned from V8!");
                }

                let name = wasm_exporttype_name(e as *const _);
                let name = std::slice::from_raw_parts((*name).data as *const u8, (*name).size);
                let name_str = String::from_utf8_lossy(name).to_string();
                let ty = IntoWasmerExternType::into_wextt(wasm_exporttype_type(e as *const _));
                if ty.is_err() {
                    panic!("{}", ty.unwrap_err());
                }

                let ty = ty.unwrap();
                wasmer_exports.push(ExportType::new(&name_str, ty))
            }

            wasmer_exports
        };
        let len = exports.len();
        wasmer_types::ExportsIterator::new(Box::new(exports.into_iter()), len)
    }

    pub fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        Box::new(vec![].into_iter())
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        panic!("no info for V8 modules")
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
