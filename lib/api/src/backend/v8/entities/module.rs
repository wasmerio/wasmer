//! Data types, functions and traits for `v8` runtime's `Module` implementation.
use std::{path::Path, sync::Arc};

use crate::{AsEngineRef, BackendModule, IntoBytes, Store, backend::v8::bindings::*};

use bytes::Bytes;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ImportType, ImportsIterator,
    ModuleInfo, SerializeError,
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
                wasm_module_delete(handle);
                return Err(SerializeError::Generic(String::from(
                    "V8 returned an empty vector as serialized module",
                )));
            }
            let serialized_bytes =
                std::slice::from_raw_parts(bytes.data as *mut u8, bytes.size).to_vec();
            wasm_byte_vec_delete(&mut bytes);
            wasm_module_delete(handle);
            serialized_bytes
        };

        Ok(bytes)
    }
}

impl Drop for ModuleHandle {
    fn drop(&mut self) {
        unsafe { wasm_shared_module_delete(self.v8_shared_module_handle) }
    }
}

const DYLINK_SECTION_NAME: &str = "dylink.0";

#[derive(Clone, Debug, PartialEq, Eq, RkyvSerialize, RkyvDeserialize, Archive)]
pub(crate) struct V8ModuleInfo {
    name: Option<String>,
    imports: Vec<ImportType>,
    exports: Vec<ExportType>,
    v8_module_bytes: Vec<u8>,
    // Copy of the section data needed by the dynamic linker, since the current
    // API cannot retrieve it later from the handle.
    dylink_section_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `module` in the `v8` runtime.
pub struct Module {
    pub(crate) handle: Arc<ModuleHandle>,
    info: V8ModuleInfo,
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
        let v8_module_bytes = module.serialize().map_err(|err| {
            CompileError::Codegen(format!(
                "Failed to serialize V8 module after compilation: {err}"
            ))
        })?;

        Ok(Self {
            handle: Arc::new(module),
            info: V8ModuleInfo {
                name: info.name,
                imports,
                exports,
                v8_module_bytes,
                dylink_section_data,
            },
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
        self.info.name.as_ref().map(|s| s.as_ref())
    }

    #[tracing::instrument(skip(self))]
    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        let info = rkyv::to_bytes::<rkyv::rancor::Error>(&self.info)
            .map_err(|err| SerializeError::Generic(format!("{err:?}")))?
            .to_vec();
        Ok(info.into())
    }

    #[allow(clippy::arc_with_non_send_sync)]
    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        tracing::info!("Creating module from deserialize_unchecked");
        let binary = bytes.into_bytes();
        dbg!(binary.len());
        let info = rkyv::access::<ArchivedV8ModuleInfo, rkyv::rancor::Error>(&binary)
            .and_then(|archived| rkyv::deserialize::<V8ModuleInfo, rkyv::rancor::Error>(archived))
            .map_err(|err| DeserializeError::CorruptedBinary(format!("{err:?}")))?;
        let module = ModuleHandle::deserialize(engine, &info.v8_module_bytes)?;

        Ok(Self {
            handle: Arc::new(module),
            info,
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
        self.info.name = Some(name.to_string());
        true
    }

    pub fn imports<'a>(&'a self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + 'a>> {
        let imports = self.info.imports.clone();
        let len = imports.len();
        wasmer_types::ImportsIterator::new(Box::new(imports.into_iter()), len)
    }

    pub fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        let exports = self.info.exports.clone();
        let len = exports.len();
        wasmer_types::ExportsIterator::new(Box::new(exports.into_iter()), len)
    }

    pub fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        if name == DYLINK_SECTION_NAME {
            Box::new(
                self.info
                    .dylink_section_data
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
