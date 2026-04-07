//! Data types, functions and traits for `wasmi`'s `Module` implementation.
use std::{path::Path, sync::Arc};

use crate::{
    AsEngineRef, BackendModule, IntoBytes,
};

use bytes::Bytes;
use ::wasmi;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ExternType, FunctionType,
    GlobalType, ImportType, ImportsIterator, MemoryType, ModuleInfo, Mutability, Pages,
    SerializeError, TableType, Type,
};
pub(crate) struct ModuleHandle {
    pub(crate) inner: wasmi::Module,
}

impl PartialEq for ModuleHandle {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(&self.inner, &other.inner)
    }
}

impl Eq for ModuleHandle {}

impl ModuleHandle {
    fn new(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let inner = wasmi::Module::new(&engine.as_engine_ref().engine().as_wasmi().inner.engine, binary)
            .map_err(|err| CompileError::Validate(err.to_string()))?;
        Ok(Self { inner })
    }
}

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `module` in `wasmi`.
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

    #[allow(clippy::arc_with_non_send_sync)]
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
        wasmi::Module::validate(
            &engine.as_engine_ref().engine().as_wasmi().inner.engine,
            binary,
        )
        .map_err(|err| CompileError::Validate(err.to_string()))
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
    /// Consume [`self`] into a reference [`crate::backend::wasmi::module::Module`].
    pub fn into_wasmi(self) -> crate::backend::wasmi::module::Module {
        match self.0 {
            BackendModule::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` module!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::wasmi::module::Module`].
    pub fn as_wasmi(&self) -> &crate::backend::wasmi::module::Module {
        match &self.0 {
            BackendModule::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` module!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::wasmi::module::Module`].
    pub fn as_wasmi_mut(&mut self) -> &mut crate::backend::wasmi::module::Module {
        match &mut self.0 {
            BackendModule::Wasmi(s) => s,
            _ => panic!("Not a `wasmi` module!"),
        }
    }
}
