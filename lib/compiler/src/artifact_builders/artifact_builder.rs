//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use crate::translator::analyze_readonly_funcref_table;
use crate::{
    ArtifactCreate, Features,
    types::{
        function::Compilation,
        module::{ArchivedCompileModuleInfo, CompileModuleInfo},
    },
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
    pub(crate) compilation: Compilation,
    pub(crate) compile_info: CompileModuleInfo,
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
        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());
        let mut compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features,
            memory_styles,
            table_styles,
            function_max_stack_usage: PrimaryMap::new(),
            data_initializers: translation
                .data_initializers
                .iter()
                .map(OwnedDataInitializer::new)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            cpu_features: cpu_features.as_u64(),
        };
        let compilation = compiler.compile_module(
            target,
            &mut compile_info,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
        )?;

        Ok(Self {
            compilation,
            compile_info,
        })
    }

    /// The maximum stack allocation directly connected to the function itself
    /// if tracked (does not include any potential function calls).
    /// Available only for the Singlepass compiler
    pub fn get_function_max_stack_usage(
        &self,
    ) -> Option<&PrimaryMap<LocalFunctionIndex, Option<usize>>> {
        Some(&self.compile_info.function_max_stack_usage)
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuild {
    type OwnedDataInitializer = &'a OwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, OwnedDataInitializer>;

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
        EnumSet::from_u64(self.compile_info.cpu_features)
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.compile_info.data_initializers.iter()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.compile_info.table_styles
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.compilation.clone())
    }
}

/// Module loaded from an archive. Since `CompileModuleInfo` is part of the public
/// interface of this crate and has to be mutable, it has to be deserialized completely.
#[derive(Debug)]
pub struct ModuleFromArchive<'a> {
    /// The compiled native object bytes
    pub compilation: &'a [u8],
    /// Data initializers
    pub data_initializers: &'a rkyv::Archived<Box<[OwnedDataInitializer]>>,
    /// CPU Feature flags for this compilation
    pub cpu_features: u64,
}

impl<'a> ModuleFromArchive<'a> {
    /// Create a module view from archived metadata and a native container.
    pub fn from_parts(module: &'a ArchivedCompileModuleInfo, compilation: &'a [u8]) -> Self {
        Self {
            compilation,
            data_initializers: &module.data_initializers,
            cpu_features: module.cpu_features.to_native(),
        }
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
        ) -> Result<&ArchivedCompileModuleInfo, DeserializeError>,
    ) -> Result<Self, DeserializeError> {
        let mut compile_info = MaybeUninit::uninit();

        let cell = ArtifactBuildFromArchiveCell::try_new(buffer, |buffer| {
            let module = module_builder(buffer)?;
            compile_info = MaybeUninit::new(
                rkyv::deserialize::<_, RkyvError>(module)
                    .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))?,
            );
            Ok::<_, DeserializeError>(ModuleFromArchive::from_parts(module, buffer.as_ref()))
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
        self.cell.borrow_dependent().compilation
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
