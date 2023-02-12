use crate::engine::AsEngineRef;
use bytes::Bytes;
use std::path::Path;
use std::sync::Arc;
use wasmer_compiler::Artifact;
use wasmer_compiler::ArtifactCreate;
use wasmer_types::{
    CompileError, DeserializeError, ExportsIterator, ImportsIterator, ModuleInfo, SerializeError,
};
use wasmer_types::{ExportType, ImportType};

use crate::{AsStoreMut, AsStoreRef, InstantiationError, IntoBytes};
use wasmer_vm::VMInstance;

/// A WebAssembly Module contains stateless WebAssembly
/// code that has already been compiled and can be instantiated
/// multiple times.
///
/// ## Cloning a module
///
/// Cloning a module is cheap: it does a shallow copy of the compiled
/// contents rather than a deep copy.
#[derive(Clone)]
pub struct Module {
    // The field ordering here is actually significant because of the drop
    // order: we want to drop the artifact before dropping the engine.
    //
    // The reason for this is that dropping the Artifact will de-register the
    // trap handling metadata from the global registry. This must be done before
    // the code memory for the artifact is freed (which happens when the store
    // is dropped) since there is a chance that this memory could be reused by
    // another module which will try to register its own trap information.
    //
    // Note that in Rust, the drop order for struct fields is from top to
    // bottom: the opposite of C++.
    //
    // In the future, this code should be refactored to properly describe the
    // ownership of the code and its metadata.
    artifact: Arc<Artifact>,
    module_info: Arc<ModuleInfo>,
}

impl Module {
    pub(crate) fn from_binary(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        Self::validate(engine, binary)?;
        unsafe { Self::from_binary_unchecked(engine, binary) }
    }

    pub(crate) unsafe fn from_binary_unchecked(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        let module = Self::compile(engine, binary)?;
        Ok(module)
    }

    pub(crate) fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        engine.as_engine_ref().engine().validate(binary)
    }

    #[cfg(feature = "compiler")]
    fn compile(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let artifact = engine.as_engine_ref().engine().compile(binary)?;
        Ok(Self::from_artifact(artifact))
    }

    #[cfg(not(feature = "compiler"))]
    fn compile(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::UnsupportedTarget(
            "The compiler feature is not enabled, but is required to compile a Module",
        ))
    }

    pub(crate) fn serialize(&self) -> Result<Bytes, SerializeError> {
        self.artifact.serialize().map(|bytes| bytes.into())
    }

    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        let bytes = bytes.into_bytes();
        let artifact = engine.as_engine_ref().engine().deserialize(&bytes)?;
        Ok(Self::from_artifact(artifact))
    }

    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let artifact = engine
            .as_engine_ref()
            .engine()
            .deserialize_from_file(path.as_ref())?;
        Ok(Self::from_artifact(artifact))
    }

    fn from_artifact(artifact: Arc<Artifact>) -> Self {
        Self {
            module_info: Arc::new(artifact.create_module_info()),
            artifact,
        }
    }

    pub(crate) fn instantiate(
        &self,
        store: &mut impl AsStoreMut,
        imports: &[crate::Extern],
    ) -> Result<VMInstance, InstantiationError> {
        // Ensure all imports come from the same context.
        for import in imports {
            if !import.is_from_store(store) {
                return Err(InstantiationError::DifferentStores);
            }
        }
        let mut store_mut = store.as_store_mut();
        let (engine, objects) = store_mut.engine_and_objects_mut();
        unsafe {
            let mut instance_handle = self.artifact.instantiate(
                engine.tunables(),
                &imports
                    .iter()
                    .map(crate::Extern::to_vm_extern)
                    .collect::<Vec<_>>(),
                objects,
            )?;

            // After the instance handle is created, we need to initialize
            // the data, call the start function and so. However, if any
            // of this steps traps, we still need to keep the instance alive
            // as some of the Instance elements may have placed in other
            // instance tables.
            self.artifact.finish_instantiation(
                store.as_store_ref().signal_handler(),
                &mut instance_handle,
            )?;

            Ok(instance_handle)
        }
    }

    pub(crate) fn name(&self) -> Option<&str> {
        self.module_info.name.as_deref()
    }

    pub(crate) fn set_name(&mut self, name: &str) -> bool {
        Arc::get_mut(&mut self.module_info).map_or(false, |mut module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    pub(crate) fn imports(&self) -> ImportsIterator<impl Iterator<Item = ImportType> + '_> {
        self.module_info.imports()
    }

    pub(crate) fn exports(&self) -> ExportsIterator<impl Iterator<Item = ExportType> + '_> {
        self.module_info.exports()
    }

    pub(crate) fn custom_sections<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = Box<[u8]>> + 'a {
        self.module_info.custom_sections(name)
    }

    pub(crate) fn info(&self) -> &ModuleInfo {
        &self.module_info
    }
}
