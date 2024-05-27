use super::bindings::wasm_module_t;
use crate::bindings::wasm_byte_vec_new;
use crate::bindings::wasm_byte_vec_new_empty;
use crate::bindings::wasm_byte_vec_t;
use crate::bindings::wasm_module_delete;
use crate::bindings::wasm_module_new;
use crate::bindings::wasm_store_t;
use crate::errors::InstantiationError;
use crate::errors::RuntimeError;
use crate::imports::Imports;
use crate::store::AsStoreMut;
use crate::store::AsStoreRef;
use crate::vm::VMInstance;
use crate::Extern;
use crate::IntoBytes;
use crate::{AsEngineRef, ExportType, ImportType};
use bytes::Bytes;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};
use wasmer_types::{
    CompileError, DeserializeError, ExportsIterator, ExternType, FunctionType, GlobalType,
    ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages, SerializeError, TableType, Type,
};

#[derive(PartialEq, Eq)]
pub(crate) struct ModuleHandle(pub(crate) *mut wasm_module_t);

impl ModuleHandle {
    fn new(store: *mut wasm_store_t, binary: &[u8]) -> Result<Self, CompileError> {
        let bytes = unsafe {
            let mut vec = wasm_byte_vec_t {
                size: 0,
                data: std::ptr::null_mut(),
                num_elems: 0,
                size_of_elem: 0,
                lock: std::ptr::null_mut(),
            };
            wasm_byte_vec_new_empty(&mut vec);
            wasm_byte_vec_new(&mut vec, binary.len(), binary.as_ptr() as _);
            &mut vec as *const _
        };

        let module = unsafe { wasm_module_new(store, bytes) };
        if module.is_null() {
            return Err(CompileError::Validate(format!("module is null")));
        }
        Ok(ModuleHandle(module))
    }
}
impl Drop for ModuleHandle {
    fn drop(&mut self) {
        unsafe { wasm_module_delete(self.0) }
    }
}

#[derive(Clone, PartialEq, Eq)]
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
        let store = engine.maybe_as_store().expect(
            "You need to pass a reference to a store (not an engine), to work with the wasm-c-api",
        );
        let binary = binary.into_bytes();
        let module = ModuleHandle::new(store.inner.store.inner, &binary)?;

        // The module is now validated, so we can safely parse its types
        let info = crate::module_info_polyfill::translate_module(&binary[..])
            .unwrap()
            .info;

        // println!("created info: {:#?}", info);

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
        unimplemented!();
    }

    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        unimplemented!();
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        unimplemented!();
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

    pub fn imports<'a>(&'a self) -> ImportsIterator<impl Iterator<Item = ImportType> + 'a> {
        self.info().imports()
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<impl Iterator<Item = ExportType> + 'a> {
        self.info().exports()
    }

    pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Box<[u8]>> + 'a {
        self.info().custom_sections(name)
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        &self.info
    }
}
