use std::{path::Path};

use bytes::Bytes;
use wasmer_types::{CompileError, DeserializeError, ExportType, ExportsIterator, ImportType, ImportsIterator, SerializeError};

use crate::{AsEngineRef, BackendModule, IntoBytes};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Module;

impl Module {
    pub(crate) fn from_binary(
        _engine: &impl AsEngineRef,
        _binary: &[u8],
    ) -> Result<Self, CompileError> {
        Err(Self::compile_error())
    }

    pub(crate) unsafe fn from_binary_unchecked(
        _engine: &impl AsEngineRef,
        _binary: &[u8],
    ) -> Result<Self, CompileError> {
        Err(Self::compile_error())
    }

    pub(crate) fn validate(
        _engine: &impl AsEngineRef,
        _binary: &[u8],
    ) -> Result<(), CompileError> {
        Err(Self::compile_error())
    }

    pub(crate) fn serialize(&self) -> Result<Bytes, SerializeError> {
        Err(SerializeError::Generic(
            "The stub backend cannot serialize modules".to_string(),
        ))
    }

    pub(crate) unsafe fn deserialize_unchecked(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        Err(Self::deserialize_error())
    }

    pub(crate) unsafe fn deserialize(
        _engine: &impl AsEngineRef,
        _bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        Err(Self::deserialize_error())
    }

    pub(crate) unsafe fn deserialize_from_file_unchecked(
        _engine: &impl AsEngineRef,
        _path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        Err(Self::deserialize_error())
    }

    pub(crate) unsafe fn deserialize_from_file(
        _engine: &impl AsEngineRef,
        _path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        Err(Self::deserialize_error())
    }

    pub(crate) fn set_name(&mut self, _name: &str) -> bool {
        panic!("stub backend cannot name modules")
    }

    pub(crate) fn name(&self) -> Option<&str> {
        panic!("stub backend does not expose module names")
    }

    pub(crate) fn imports<'a>(&'a self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + 'a>> {
        panic!("stub backend does not expose module imports")
    }

    pub(crate) fn exports<'a>(&'a self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + 'a>> {
        panic!("stub backend does not expose module exports")
    }

    pub(crate) fn custom_sections<'a>(
        &'a self,
        _name: &'a str,
    ) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a> {
        panic!("stub backend does not expose module custom sections")
    }

    pub(crate) fn info(&self) -> ! {
        panic!("stub backend does not expose module info")
    }

    fn compile_error() -> CompileError {
        CompileError::UnsupportedTarget(
            "No runtime backend is enabled; the stub backend cannot compile modules".to_string(),
        )
    }

    fn deserialize_error() -> DeserializeError {
        DeserializeError::Generic(
            "No runtime backend is enabled; the stub backend cannot deserialize modules".to_string(),
        )
    }
}

impl crate::Module {
    pub fn into_stub(self) -> crate::backend::stub::entities::module::Module {
        match self.0 {
            BackendModule::Stub(s) => s,
            _ => panic!("Not a stub module!"),
        }
    }

    pub fn as_stub(&self) -> &crate::backend::stub::entities::module::Module {
        match self.0 {
            BackendModule::Stub(ref s) => s,
            _ => panic!("Not a stub module!"),
        }
    }

    pub fn as_stub_mut(&mut self) -> &mut crate::backend::stub::entities::module::Module {
        match self.0 {
            BackendModule::Stub(ref mut s) => s,
            _ => panic!("Not a stub module!"),
        }
    }
}
