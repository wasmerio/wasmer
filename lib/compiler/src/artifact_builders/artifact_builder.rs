//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use crate::translator::analyze_readonly_funcref_table;
use crate::{
    ArtifactCreate, Features,
    serialize::{ArchivedSerializableModule, SerializableModule},
    types::module::CompileModuleInfo,
};
#[cfg(feature = "compiler")]
use crate::{EngineInner, ModuleEnvironment, ModuleMiddlewareChain};
#[cfg(feature = "compiler")]
use wasmer_types::{CompilationProgressCallback, target::Target};

use core::mem::MaybeUninit;
use enumset::EnumSet;
use rkyv::rancor::Error as RkyvError;
use self_cell::self_cell;
use shared_buffer::OwnedBuffer;
use std::sync::Arc;
use wasmer_types::{DeserializeError, entity::PrimaryMap, target::CpuFeature};

// Not every compiler backend uses these.
#[allow(unused)]
use wasmer_types::*;

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct ArtifactBuild {
    pub(crate) serializable: SerializableModule,
}

impl ArtifactBuild {
    /// Check if the provided bytes look like a serialized `ArtifactBuild`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(&object::elf::ELFMAG)
            || bytes.starts_with(&object::macho::MH_MAGIC_64.to_le_bytes())
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
        use crate::types::function::Compilation;

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
            function_max_stack_usage: PrimaryMap::new(),
        };
        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());
        let mut serializable = SerializableModule {
            // The native image does not exist yet. This placeholder is only
            // used for the metadata copy embedded in that image.
            compilation: Vec::new(),
            compile_info,
            data_initializers: translation
                .data_initializers
                .iter()
                .map(OwnedDataInitializer::new)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            cpu_features: cpu_features.as_u64(),
        };
        let compile_info_blob = serializable
            .serialize()
            .map_err(|e| CompileError::Codegen(format!("cannot serialize SerializeModule: {e}")))?;

        let compilation = compiler.compile_module(
            target,
            &serializable.compile_info,
            &compile_info_blob,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
        )?;

        let Compilation {
            data,
            function_max_stack_usage,
        } = compilation;
        serializable.compile_info.function_max_stack_usage = function_max_stack_usage;
        serializable.compilation = data;
        Ok(Self { serializable })
    }

    /// Create a new ArtifactBuild from a SerializableModule
    pub fn from_serializable(serializable: SerializableModule) -> Self {
        Self { serializable }
    }

    /// The maximum stack allocation directly connected to the function itself
    /// if tracked (does not include any potential function calls).
    /// Available only for the Singlepass compiler
    pub fn get_function_max_stack_usage(
        &self,
    ) -> Option<&PrimaryMap<LocalFunctionIndex, Option<usize>>> {
        Some(&self.serializable.compile_info.function_max_stack_usage)
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

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.serializable.data_initializers.iter()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.serializable.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.serializable.compile_info.table_styles
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.serializable.compilation.clone())
    }
}

/// Module loaded from an archive. Since `CompileModuleInfo` is part of the public
/// interface of this crate and has to be mutable, it has to be deserialized completely.
#[derive(Debug)]
pub struct ModuleFromArchive<'a> {
    /// The compiled native object bytes
    pub compilation: &'a rkyv::Archived<Vec<u8>>,
    /// Data initializers
    pub data_initializers: &'a rkyv::Archived<Box<[OwnedDataInitializer]>>,
    /// CPU Feature flags for this compilation
    pub cpu_features: u64,
}

impl<'a> ModuleFromArchive<'a> {
    /// Create a new `ModuleFromArchive` from the archived version of a `SerializableModule`
    pub fn from_serializable_module(
        module: &'a ArchivedSerializableModule,
    ) -> Result<Self, DeserializeError> {
        Ok(Self {
            compilation: &module.compilation,
            data_initializers: &module.data_initializers,
            cpu_features: module.cpu_features.to_native(),
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

    /// The maximum stack allocation directly connected to the function itself
    /// if tracked (does not include any potential function calls).
    /// Available only for the Singlepass compiler
    pub fn get_function_max_stack_usage(
        &self,
    ) -> Option<&PrimaryMap<LocalFunctionIndex, Option<usize>>> {
        Some(&self.compile_info.function_max_stack_usage)
    }

    /// Get compiled native object data.
    pub fn get_elf_file(&self) -> &[u8] {
        self.cell.borrow_dependent().compilation.as_slice()
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

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.cell.borrow_dependent().data_initializers.iter()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.compile_info.table_styles
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.get_elf_file().to_vec())
    }
}
