//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use super::trampoline::{libcall_trampoline_len, make_libcall_trampolines};

#[cfg(feature = "compiler")]
use crate::{
    serialize::SerializableCompilation, types::target::Target, EngineInner, ModuleEnvironment,
    ModuleMiddlewareChain,
};
use crate::{
    serialize::{
        ArchivedSerializableCompilation, ArchivedSerializableModule, MetadataHeader,
        SerializableModule,
    },
    types::{
        function::{CompiledFunctionFrameInfo, FunctionBody, UnwindInfo, GOT},
        module::CompileModuleInfo,
        relocation::Relocation,
        section::{CustomSection, SectionIndex},
        target::CpuFeature,
    },
    ArtifactCreate, Features,
};
use core::mem::MaybeUninit;
use enumset::EnumSet;
use rkyv::rancor::Error as RkyvError;
use self_cell::self_cell;
use shared_buffer::OwnedBuffer;
use std::sync::Arc;
use wasmer_types::{
    entity::{ArchivedPrimaryMap, PrimaryMap},
    DeserializeError,
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
        hash_algorithm: Option<HashAlgorithm>,
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

        if let Some(hash_algorithm) = hash_algorithm {
            let hash = match hash_algorithm {
                HashAlgorithm::Sha256 => ModuleHash::sha256(data),
                HashAlgorithm::XXHash => ModuleHash::xxhash(data),
            };

            module.hash = Some(hash);
        }

        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features,
            memory_styles,
            table_styles,
        };

        // Compile the Module
        let compilation = compiler.compile_module(
            target,
            &compile_info,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
        )?;

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        // Synthesize a custom section to hold the libcall trampolines.
        let mut function_frame_info = PrimaryMap::with_capacity(compilation.functions.len());
        let mut function_bodies = PrimaryMap::with_capacity(compilation.functions.len());
        let mut function_relocations = PrimaryMap::with_capacity(compilation.functions.len());
        for (_, func) in compilation.functions.into_iter() {
            function_bodies.push(func.body);
            function_relocations.push(func.relocations);
            function_frame_info.push(func.frame_info);
        }
        let mut custom_sections = compilation.custom_sections.clone();
        let mut custom_section_relocations = compilation
            .custom_sections
            .iter()
            .map(|(_, section)| section.relocations.clone())
            .collect::<PrimaryMap<SectionIndex, _>>();
        let libcall_trampolines_section = make_libcall_trampolines(target);
        custom_section_relocations.push(libcall_trampolines_section.relocations.clone());
        let libcall_trampolines = custom_sections.push(libcall_trampolines_section);
        let libcall_trampoline_len = libcall_trampoline_len(target) as u32;
        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());

        let serializable_compilation = SerializableCompilation {
            function_bodies,
            function_relocations,
            function_frame_info,
            function_call_trampolines: compilation.function_call_trampolines,
            dynamic_function_trampolines: compilation.dynamic_function_trampolines,
            custom_sections,
            custom_section_relocations,
            unwind_info: compilation.unwind_info,
            libcall_trampolines,
            libcall_trampoline_len,
            got: compilation.got,
        };
        let serializable = SerializableModule {
            compilation: serializable_compilation,
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

    /// Get Functions Bodies ref
    pub fn get_function_bodies_ref(&self) -> &PrimaryMap<LocalFunctionIndex, FunctionBody> {
        &self.serializable.compilation.function_bodies
    }

    /// Get Functions Call Trampolines ref
    pub fn get_function_call_trampolines_ref(&self) -> &PrimaryMap<SignatureIndex, FunctionBody> {
        &self.serializable.compilation.function_call_trampolines
    }

    /// Get Dynamic Functions Call Trampolines ref
    pub fn get_dynamic_function_trampolines_ref(&self) -> &PrimaryMap<FunctionIndex, FunctionBody> {
        &self.serializable.compilation.dynamic_function_trampolines
    }

    /// Get Custom Sections ref
    pub fn get_custom_sections_ref(&self) -> &PrimaryMap<SectionIndex, CustomSection> {
        &self.serializable.compilation.custom_sections
    }

    /// Get Function Relocations
    pub fn get_function_relocations(&self) -> &PrimaryMap<LocalFunctionIndex, Vec<Relocation>> {
        &self.serializable.compilation.function_relocations
    }

    /// Get Function Relocations ref
    pub fn get_custom_section_relocations_ref(&self) -> &PrimaryMap<SectionIndex, Vec<Relocation>> {
        &self.serializable.compilation.custom_section_relocations
    }

    /// Get LibCall Trampoline Section Index
    pub fn get_libcall_trampolines(&self) -> SectionIndex {
        self.serializable.compilation.libcall_trampolines
    }

    /// Get LibCall Trampoline Length
    pub fn get_libcall_trampoline_len(&self) -> usize {
        self.serializable.compilation.libcall_trampoline_len as usize
    }

    /// Get a reference to the [`UnwindInfo`].
    pub fn get_unwind_info(&self) -> &UnwindInfo {
        &self.serializable.compilation.unwind_info
    }

    /// Get a reference to the [`GOT`].
    pub fn get_got_ref(&self) -> &GOT {
        &self.serializable.compilation.got
    }

    /// Get Function Relocations ref
    pub fn get_frame_info_ref(&self) -> &PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo> {
        &self.serializable.compilation.function_frame_info
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuild {
    type OwnedDataInitializer = &'a OwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, OwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.serializable.compile_info.module.clone()
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.serializable.compile_info.module).map_or(false, |module_info| {
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
        serialize_module(&self.serializable)
    }
}

/// Module loaded from an archive. Since `CompileModuleInfo` is part of the public
/// interface of this crate and has to be mutable, it has to be deserialized completely.
#[derive(Debug)]
pub struct ModuleFromArchive<'a> {
    /// The main serializable compilation object
    pub compilation: &'a ArchivedSerializableCompilation,
    /// Datas initializers
    pub data_initializers: &'a rkyv::Archived<Box<[OwnedDataInitializer]>>,
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
            compilation: &module.compilation,
            data_initializers: &module.data_initializers,
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

    /// Compilation informations
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

    /// Get Functions Bodies ref
    pub fn get_function_bodies_ref(&self) -> &ArchivedPrimaryMap<LocalFunctionIndex, FunctionBody> {
        &self.cell.borrow_dependent().compilation.function_bodies
    }

    /// Get Functions Call Trampolines ref
    pub fn get_function_call_trampolines_ref(
        &self,
    ) -> &ArchivedPrimaryMap<SignatureIndex, FunctionBody> {
        &self
            .cell
            .borrow_dependent()
            .compilation
            .function_call_trampolines
    }

    /// Get Dynamic Functions Call Trampolines ref
    pub fn get_dynamic_function_trampolines_ref(
        &self,
    ) -> &ArchivedPrimaryMap<FunctionIndex, FunctionBody> {
        &self
            .cell
            .borrow_dependent()
            .compilation
            .dynamic_function_trampolines
    }

    /// Get Custom Sections ref
    pub fn get_custom_sections_ref(&self) -> &ArchivedPrimaryMap<SectionIndex, CustomSection> {
        &self.cell.borrow_dependent().compilation.custom_sections
    }

    /// Get Function Relocations
    pub fn get_function_relocations(
        &self,
    ) -> &ArchivedPrimaryMap<LocalFunctionIndex, Vec<Relocation>> {
        &self
            .cell
            .borrow_dependent()
            .compilation
            .function_relocations
    }

    /// Get Function Relocations ref
    pub fn get_custom_section_relocations_ref(
        &self,
    ) -> &ArchivedPrimaryMap<SectionIndex, Vec<Relocation>> {
        &self
            .cell
            .borrow_dependent()
            .compilation
            .custom_section_relocations
    }

    /// Get LibCall Trampoline Section Index
    pub fn get_libcall_trampolines(&self) -> SectionIndex {
        rkyv::deserialize::<_, RkyvError>(
            &self.cell.borrow_dependent().compilation.libcall_trampolines,
        )
        .unwrap()
    }

    /// Get LibCall Trampoline Length
    pub fn get_libcall_trampoline_len(&self) -> usize {
        self.cell
            .borrow_dependent()
            .compilation
            .libcall_trampoline_len
            .to_native() as usize
    }

    /// Get an unarchived [`UnwindInfo`].
    pub fn get_unwind_info(&self) -> UnwindInfo {
        rkyv::deserialize::<_, rkyv::rancor::Error>(
            &self.cell.borrow_dependent().compilation.unwind_info,
        )
        .unwrap()
    }

    /// Get an unarchived [`GOT`].
    pub fn get_got_ref(&self) -> GOT {
        rkyv::deserialize::<_, rkyv::rancor::Error>(&self.cell.borrow_dependent().compilation.got)
            .unwrap()
    }

    /// Get Function Relocations ref
    pub fn get_frame_info_ref(
        &self,
    ) -> &ArchivedPrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo> {
        &self.cell.borrow_dependent().compilation.function_frame_info
    }

    /// Get Function Relocations ref
    pub fn deserialize_frame_info_ref(
        &self,
    ) -> Result<PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>, DeserializeError> {
        rkyv::deserialize::<_, RkyvError>(
            &self.cell.borrow_dependent().compilation.function_frame_info,
        )
        .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuildFromArchive {
    type OwnedDataInitializer = &'a ArchivedOwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, ArchivedOwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.compile_info.module.clone()
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.compile_info.module).map_or(false, |module_info| {
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
