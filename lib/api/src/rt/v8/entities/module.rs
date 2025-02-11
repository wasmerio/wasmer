//! Data types, functions and traits for `v8` runtime's `Module` implementation.
use std::{path::Path, sync::Arc};

use crate::{rt::v8::bindings::*, AsEngineRef, IntoBytes, RuntimeModule};

use bytes::Bytes;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ExternType, FunctionType,
    GlobalType, ImportType, ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages,
    SerializeError, TableType, Type,
};

pub(crate) struct ModuleHandle {
    pub(crate) inner: *mut wasm_shared_module_t,
    pub(crate) store: std::sync::Mutex<crate::store::Store>,
}

impl PartialEq for ModuleHandle {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
        // && self.store.lock() == other.store.lock()
    }
}

impl Eq for ModuleHandle {}

impl ModuleHandle {
    fn new(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let bytes = wasm_byte_vec_t {
            size: binary.len(),
            data: binary.as_ptr() as _,
        };

        let store = crate::store::Store::new(engine.as_engine_ref().engine().clone());

        let inner = unsafe { wasm_module_new(store.inner.store.as_v8().inner, &bytes as *const _) };

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

        let store = std::sync::Mutex::new(store);
        Ok(ModuleHandle { inner, store })
    }
}
impl Drop for ModuleHandle {
    fn drop(&mut self) {
        unsafe { wasm_shared_module_delete(self.inner) }
    }
}

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `module` in the `v8` runtime.
pub struct Module {
    pub(crate) handle: Arc<ModuleHandle>,
    name: Option<String>,
    raw_bytes: Option<Bytes>,
    info: ModuleInfo,
}

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
        let binary = binary.into_bytes();
        let module = ModuleHandle::new(engine, &binary)?;
        let info = crate::utils::polyfill::translate_module(&binary[..])
            .unwrap()
            .info;

        Ok(Self {
            handle: Arc::new(module),
            name: info.name.clone(),
            raw_bytes: Some(binary.into_bytes()),
            info,
        })
    }

    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        let engine = engine.as_engine_ref();
        unimplemented!();
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
    }

    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        return self.raw_bytes.clone().ok_or(SerializeError::Generic(
            "Not able to serialize module".to_string(),
        ));
    }

    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        Self::deserialize(engine, bytes)
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        return Self::from_binary(engine, &bytes.into_bytes())
            .map_err(|e| DeserializeError::Compiler(e));
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
    /// Consume [`self`] into a reference [`crate::rt::v8::module::Module`].
    pub fn into_v8(self) -> crate::rt::v8::module::Module {
        match self.0 {
            RuntimeModule::V8(s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::v8::module::Module`].
    pub fn as_v8(&self) -> &crate::rt::v8::module::Module {
        match self.0 {
            RuntimeModule::V8(ref s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::v8::module::Module`].
    pub fn as_v8_mut(&mut self) -> &mut crate::rt::v8::module::Module {
        match self.0 {
            RuntimeModule::V8(ref mut s) => s,
            _ => panic!("Not a `v8` module!"),
        }
    }
}
