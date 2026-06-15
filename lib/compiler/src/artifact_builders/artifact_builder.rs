//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use super::trampoline::{libcall_trampoline_len, make_libcall_trampolines};
#[cfg(feature = "compiler")]
use crate::translator::analyze_readonly_funcref_table;
use crate::{
    ArtifactCreate, Features,
    serialize::{
        ArchivedSerializableCompilation, ArchivedSerializableModule, MetadataHeader,
        SerializableModule,
    },
    types::{
        function::{CompiledFunctionFrameInfo, FunctionBody, GOT, UnwindInfo},
        module::CompileModuleInfo,
        relocation::Relocation,
        section::{CustomSection, SectionIndex},
    },
};
#[cfg(feature = "compiler")]
use crate::{
    EngineInner, ModuleEnvironment, ModuleMiddlewareChain, serialize::SerializableCompilation,
};
#[cfg(feature = "compiler")]
use wasmer_types::{CompilationProgressCallback, target::Target};

use core::mem::MaybeUninit;
use enumset::EnumSet;
use rkyv::rancor::Error as RkyvError;
use self_cell::self_cell;
use shared_buffer::OwnedBuffer;
use std::sync::Arc;
use wasmer_types::{
    DeserializeError,
    entity::{ArchivedPrimaryMap, PrimaryMap},
    target::CpuFeature,
};

// Not every compiler backend uses these.
#[allow(unused)]
use wasmer_types::*;

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct ArtifactBuild {
    serializable: SerializableModule,
}

impl ArtifactBuild {
    /// Header signature for wasmu binary
    pub const MAGIC_HEADER: &'static [u8; 16] = b"wasmer-universal";

    /// Check if the provided bytes look like a serialized `ArtifactBuild`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(Self::MAGIC_HEADER)
    }

    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        inner_engine: &mut EngineInner,
        data: &[u8],
        target: &Target,
        memory_styles: PrimaryMap<MemoryIndex, MemoryStyle>,
        table_styles: PrimaryMap<TableIndex, TableStyle>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let features = inner_engine.features().clone();

        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        let compiler = inner_engine.compiler()?;

        // We try to apply the middleware first
        let mut module = translation.module;
        let middlewares = compiler.get_middlewares();
        middlewares
            .apply_on_module_info(&mut module)
            .map_err(|err| CompileError::MiddlewareError(err.to_string()))?;
        #[cfg(feature = "translator")]
        if compiler.enable_readonly_funcref_table()
            && let Some(table_index) =
                analyze_readonly_funcref_table(&module, &translation.function_body_inputs)?
        {
            module.tables[table_index].readonly = true;
        }

        module.hash = Some(ModuleHash::new(data));
        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features,
            memory_styles,
            table_styles,
        };

        // Compile the Module
        compiler.compile_module(
            target,
            &compile_info,
            rkyv::to_bytes::<rkyv::rancor::Error>(&compile_info)
                // TODO
                .map(|bytes| bytes.into_vec())
                .unwrap(),
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
        )?;

        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());
        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let serializable = SerializableModule {
            compile_info,
            data_initializers,
            cpu_features: cpu_features.as_u64(),
        };
        Ok(Self { serializable })
    }

    /// Create a new ArtifactBuild from a SerializableModule
    pub fn from_serializable(serializable: SerializableModule) -> Self {
        Self { serializable }
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuild {
    type OwnedDataInitializer = &'a OwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, OwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.serializable.compile_info.module.clone()
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.serializable.compile_info.module).is_some_and(|module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    fn module_info(&self) -> &ModuleInfo {
        &self.serializable.compile_info.module
    }

    fn features(&self) -> &Features {
        &self.serializable.compile_info.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.serializable.cpu_features)
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.serializable.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.serializable.compile_info.table_styles
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.serializable.data_initializers.iter()
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        serialize_module(&self.serializable)
    }
}

/// Module loaded from an archive. Since `CompileModuleInfo` is part of the public
/// interface of this crate and has to be mutable, it has to be deserialized completely.
#[derive(Debug)]
pub struct ModuleFromArchive<'a> {
    /// CPU Feature flags for this compilation
    pub cpu_features: u64,

    // Keep the original module around for re-serialization
    original_module: &'a ArchivedSerializableModule,
}

impl<'a> ModuleFromArchive<'a> {
    /// Create a new `ModuleFromArchive` from the archived version of a `SerializableModule`
    pub fn from_serializable_module(
        module: &'a ArchivedSerializableModule,
    ) -> Result<Self, DeserializeError> {
        Ok(Self {
            cpu_features: module.cpu_features.to_native(),
            original_module: module,
        })
    }
}

self_cell!(
    struct ArtifactBuildFromArchiveCell {
        owner: OwnedBuffer,

        #[covariant]
        dependent: ModuleFromArchive,
    }

    impl {Debug}
);

#[cfg(feature = "artifact-size")]
impl loupe::MemoryUsage for ArtifactBuildFromArchiveCell {
    fn size_of_val(&self, _tracker: &mut dyn loupe::MemoryUsageTracker) -> usize {
        std::mem::size_of_val(self.borrow_owner()) + std::mem::size_of_val(self.borrow_dependent())
    }
}

/// A compiled wasm module that was loaded from a serialized archive.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct ArtifactBuildFromArchive {
    cell: Arc<ArtifactBuildFromArchiveCell>,

    /// Compilation information
    compile_info: CompileModuleInfo,
}

impl ArtifactBuildFromArchive {
    #[allow(unused)]
    pub(crate) fn try_new(
        buffer: OwnedBuffer,
        module_builder: impl FnOnce(
            &OwnedBuffer,
        ) -> Result<&ArchivedSerializableModule, DeserializeError>,
    ) -> Result<Self, DeserializeError> {
        let mut compile_info = MaybeUninit::uninit();

        let cell = ArtifactBuildFromArchiveCell::try_new(buffer, |buffer| {
            let module = module_builder(buffer)?;
            compile_info = MaybeUninit::new(
                rkyv::deserialize::<_, RkyvError>(&module.compile_info)
                    .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))?,
            );
            ModuleFromArchive::from_serializable_module(module)
        })?;

        // Safety: we know the lambda will execute before getting here and assign both values
        let compile_info = unsafe { compile_info.assume_init() };
        Ok(Self {
            cell: Arc::new(cell),
            compile_info,
        })
    }

    /// Gets the owned buffer
    pub fn owned_buffer(&self) -> &OwnedBuffer {
        self.cell.borrow_owner()
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuildFromArchive {
    type OwnedDataInitializer = &'a ArchivedOwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, ArchivedOwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.compile_info.module.clone()
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.compile_info.module).is_some_and(|module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    fn module_info(&self) -> &ModuleInfo {
        &self.compile_info.module
    }

    fn features(&self) -> &Features {
        &self.compile_info.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.cell.borrow_dependent().cpu_features)
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.compile_info.table_styles
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.cell
            .borrow_dependent()
            .original_module
            .data_initializers
            .iter()
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        // We could have stored the original bytes, but since the module info name
        // is mutable, we have to assume the data may have changed and serialize
        // everything all over again. Also, to be able to serialize, first we have
        // to deserialize completely. Luckily, serializing a module that was already
        // deserialized from a file makes little sense, so hopefully, this is not a
        // common use-case.

        let mut module: SerializableModule =
            rkyv::deserialize::<_, RkyvError>(self.cell.borrow_dependent().original_module)
                .map_err(|e| SerializeError::Generic(e.to_string()))?;
        module.compile_info = self.compile_info.clone();
        serialize_module(&module)
    }
}

fn serialize_module(module: &SerializableModule) -> Result<Vec<u8>, SerializeError> {
    let serialized_data = module.serialize()?;
    assert!(std::mem::align_of::<SerializableModule>() <= MetadataHeader::ALIGN);

    let mut metadata_binary = vec![];
    metadata_binary.extend(ArtifactBuild::MAGIC_HEADER);
    metadata_binary.extend(MetadataHeader::new(serialized_data.len()).into_bytes());
    metadata_binary.extend(serialized_data);
    Ok(metadata_binary)
}
