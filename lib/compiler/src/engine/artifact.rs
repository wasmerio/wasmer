//! Define `Artifact`, based on `ArtifactBuild`
//! to allow compiling and instantiating to be done as separate steps.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{
    ffi::c_void,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    os::fd::AsRawFd,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
};

#[cfg(feature = "compiler")]
use crate::ModuleEnvironment;
use crate::types::module::CompileModuleInfo;
#[cfg(any(feature = "static-artifact-create", feature = "static-artifact-load"))]
use crate::types::symbols::ModuleMetadata;
#[cfg(feature = "static-artifact-create")]
use crate::{Compiler, FunctionBodyData, ModuleTranslationState};
use crate::{
    Engine, EngineInner, FunctionExtent, GlobalFrameInfoRegistration, InstantiationError, Tunables,
    WASMER_FUNCTION_OFFSETS_SECTION_NAME, WASMER_MODULE_INFO_SECTION_NAME,
    WASMER_TRAP_FUNCTION_OFFSETS_SECTION_NAME, WASMER_TRAPS_SECTION_NAME,
    engine::{mapped_binary::MemoryMappedBinary, resolver::resolve_tags},
    register_frame_info, resolve_imports,
    serialize::SerializableModule,
};
use itertools::Itertools;
use object::{Object, ObjectSection, ReadCache};

use tempfile::NamedTempFile;
#[cfg(feature = "static-artifact-create")]
use wasmer_types::Features;
use wasmer_types::{
    CompilationProgressCallback, CompileError, DataInitializer, DeserializeError, FunctionIndex,
    LocalFunctionIndex, MemoryIndex, ModuleInfo, OwnedDataInitializer, SerializeError,
    SignatureIndex, TableIndex, TrapCode, TrapInformation,
    entity::{BoxedSlice, EntityRef, PrimaryMap},
    target::{CpuFeature, Target},
};

use wasmer_types::VMOffsets;
use wasmer_vm::{
    FunctionBodyPtr, InstanceAllocator, MemoryStyle, StoreObjects, TableStyle, TrapHandlerFn,
    VMConfig, VMExtern, VMInstance, VMSignatureHash, VMTrampoline,
};

#[derive(Debug)]
pub(crate) enum ModuleFile {
    TempFile(NamedTempFile),
    OwnedFile(File),
}

impl ModuleFile {
    pub(crate) fn file(&mut self) -> &mut File {
        match self {
            Self::OwnedFile(file) => file,
            Self::TempFile(tempfile) => tempfile.as_file_mut(),
        }
    }

    fn try_clone_reader(&self) -> Result<BufReader<File>, io::Error> {
        match self {
            Self::OwnedFile(file) => {
                let mut clone = file.try_clone()?;
                clone.seek(io::SeekFrom::Start(0))?;
                Ok(BufReader::new(clone))
            }
            Self::TempFile(tempfile) => Ok(BufReader::new(File::open(tempfile.path())?)),
        }
    }
}

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
struct ArtifactBuild {
    serializable: SerializableModule,
    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    module_file: ModuleFile,
}

impl ArtifactBuild {
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
        use wasmer_types::ModuleHash;

        use crate::ModuleMiddlewareChain;
        #[cfg(feature = "translator")]
        use crate::translator::analyze_readonly_funcref_table;

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
        let compile_info = serializable.compile_info.clone();

        let suffix = if !cfg!(target_os = "linux") {
            ".so"
        } else {
            ".dylib"
        };
        let module_file = tempfile::Builder::new()
            .prefix("wasmer-image")
            .suffix(suffix)
            .tempfile()
            .map_err(|err| CompileError::Codegen(format!("cannot create temporary file: {err}")))?;

        // Compile the Module
        let (module_file, serializable) = compiler.compile_module(
            target,
            &compile_info,
            serializable,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
            module_file,
        )?;

        Ok(Self {
            serializable,
            module_file: ModuleFile::TempFile(module_file),
        })
    }
}

/*
TODO:
/ This shows if the frame info has been registered already or not.
    // Because the 'GlobalFrameInfoRegistration' ownership can be transferred to EngineInner
    // this bool is needed to track the status, as 'frame_info_registration' will be None
    // after the ownership is transferred.
    frame_info_registered: bool,
    // frame_info_registered is not staying there but transferred to CodeMemory from EngineInner
    // using 'Artifact::take_frame_info_registration' method
    // so the GloabelFrameInfo and MMap stays in sync and get dropped at the same time
    frame_info_registration: Option<GlobalFrameInfoRegistration>,
*/

#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct AllocatedArtifact {
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,

    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSignatureHash>,
    finished_function_lengths: BoxedSlice<LocalFunctionIndex, usize>,

    // This shows if the frame info has been registered already or not.
    // Because the 'GlobalFrameInfoRegistration' ownership can be transferred to EngineInner
    // this bool is needed to track the status, as 'frame_info_registration' will be None
    // after the ownership is transferred.
    frame_info_registered: bool,
    // frame_info_registration is transferred to CodeMemory from EngineInner
    // using 'Artifact::take_frame_info_registration' method
    // so the GlobalFrameInfo and MMap stays in sync and get dropped at the same time
    frame_info_registration: Option<GlobalFrameInfoRegistration>,

    /// Precomputed `VMOffsets` for this artifact's module, cloned by
    /// `Artifact::instantiate` instead of recomputing on every call.
    ///
    /// Safe to cache because `VMOffsets::new(pointer_size, module_info)`
    /// is deterministic, `module_info` is immutable after compile (the
    /// only mutable field `name` is not a `VMOffsets` input), and the
    /// host's pointer size is a runtime constant.
    ///
    /// Built once in `from_parts` and in the deserialization path
    /// (`deserialize_object_native`); `VMOffsets::new` was ~9% of
    /// `Instance::new` time on profile traces of a per-request wasm
    /// host calling `Module::instantiate` in a tight loop.
    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    vm_offsets: VMOffsets,

    debug_info: Arc<Mutex<addr2line::Loader>>,

    // Compiled executable mmapped into memory.
    _memory_map: MemoryMappedBinary,
}

impl AllocatedArtifact {
    fn from_binary(
        module_info: &ModuleInfo,
        module_file: &mut File,
        signatures: PrimaryMap<SignatureIndex, VMSignatureHash>,
    ) -> Result<Self, String> {
        // TODO
        let path_prefix = if cfg!(target_os = "linux") {
            PathBuf::from("/proc/self/fd")
        } else {
            PathBuf::from("/dev/fd")
        };
        let module_file_fd = module_file.as_raw_fd();
        let debug_info = addr2line::Loader::new(path_prefix.join(module_file_fd.to_string()))
            .map(|loader| Arc::new(Mutex::new(loader)))
            .map_err(|e| format!("cannot parse debug info from an artifact file: {e}"))?;
        module_file
            .seek(io::SeekFrom::Start(0))
            .map_err(|e| format!("cannot seek artifact file: {e}"))?;
        let reader = BufReader::new(module_file);
        let cache = ReadCache::new(reader);
        let image = object::File::parse(&cache).map_err(|e| format!("cannot parse image: {e}"))?;
        let mut memory_map = MemoryMappedBinary::try_from_file(module_file_fd, &image)?;

        // Parts function offsets
        let mut function_offsets = None;
        for section in image.sections() {
            let Ok(section_name) = section.name_bytes() else {
                continue;
            };
            match section_name {
                WASMER_FUNCTION_OFFSETS_SECTION_NAME => {
                    let data = section
                        .data()
                        .map_err(|e| format!("cannot load image section data: {e}"))?;
                    function_offsets = Some(
                        data.chunks_exact(size_of::<usize>())
                            .map(|chunk| {
                                let arr: [u8; 8] = chunk.try_into().unwrap();
                                usize::from_le_bytes(arr)
                            })
                            .collect_vec(),
                    );
                }
                b".eh_frame" => {
                    memory_map.publish_eh_frame_section(section.address(), section.size())?
                }
                _ => {}
            }
        }
        let Some(function_offsets) = function_offsets else {
            return Err("missing function offset section in the image".to_string());
        };
        let local_function_count = module_info.functions.len() - module_info.num_imported_functions;

        let local_fn_sizes = function_offsets
            .iter()
            .skip(1)
            .take(local_function_count)
            .zip(function_offsets.iter())
            .map(|(f1, f0)| f1 - f0)
            .collect_vec();
        let (local_fn_offsets, rest) = function_offsets
            .split_at(module_info.functions.len() - module_info.num_imported_functions);
        let (trampoline_offsets, dynamic_trampoline_offsets) =
            rest.split_at(module_info.signatures.len());
        if local_fn_offsets.len() != local_function_count
            || trampoline_offsets.len() != module_info.signatures.len()
            || dynamic_trampoline_offsets.len() != module_info.imported_function_types().count()
        {
            return Err(format!(
                "corrupted {} section",
                String::from_utf8_lossy(WASMER_FUNCTION_OFFSETS_SECTION_NAME)
            ));
        }

        let base = memory_map.base();
        Ok(Self {
            finished_functions: local_fn_offsets
                .iter()
                .map(|&offset| FunctionBodyPtr(unsafe { base.add(offset) as _ }))
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_function_call_trampolines: trampoline_offsets
                .iter()
                .map(|&offset| unsafe {
                    std::mem::transmute::<*mut c_void, VMTrampoline>(base.add(offset))
                })
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_dynamic_function_trampolines: dynamic_trampoline_offsets
                .iter()
                .map(|&offset| FunctionBodyPtr(unsafe { base.add(offset) as _ }))
                .collect::<PrimaryMap<_, _>>()
                .into_boxed_slice(),
            finished_function_lengths: PrimaryMap::from_iter(local_fn_sizes).into_boxed_slice(),
            frame_info_registered: false,
            frame_info_registration: None,
            signatures: signatures.into_boxed_slice(),
            vm_offsets: VMOffsets::new(std::mem::size_of::<usize>() as u8, module_info),
            debug_info,
            _memory_map: memory_map,
        })
    }

    fn function_extents(&self) -> PrimaryMap<LocalFunctionIndex, FunctionExtent> {
        assert_eq!(
            self.finished_functions.len(),
            self.finished_function_lengths.len(),
            "finished_functions and finished_function_lengths must have equal length"
        );
        self.finished_functions
            .iter()
            .map(|(index, &ptr)| {
                let length = self.finished_function_lengths[index];
                FunctionExtent { ptr, length }
            })
            .collect()
    }
}

/// On-demand reader of per-function trap information from the artifact's
/// object file.
///
/// Trap lookups only happen when a trap actually fires (a rare event), so
/// instead of eagerly parsing and keeping every function's trap table
/// resident in memory we re-parse the relevant object sections lazily on each
/// lookup. The reader holds its own duplicated file descriptor to the
/// artifact, independent of the artifact's primary `module_file`.
pub struct TrapReader {
    file: Mutex<File>,
}

impl TrapReader {
    fn new(file: File) -> Self {
        Self {
            file: Mutex::new(file),
        }
    }

    /// Looks up the trap information for `local_index` at `rel_pos`, the offset
    /// relative to the start of the function.
    pub fn lookup(&self, local_index: LocalFunctionIndex, rel_pos: u32) -> Option<TrapInformation> {
        let mut file = self.file.lock().ok()?;
        file.seek(SeekFrom::Start(0)).ok()?;
        let reader = BufReader::new(file.try_clone().ok()?);
        let cache = ReadCache::new(reader);
        let image = object::File::parse(&cache).ok()?;

        let mut trap_section_data = None;
        let mut trap_function_offsets = None;
        for section in image.sections() {
            let Ok(section_name) = section.name_bytes() else {
                continue;
            };
            match section_name {
                WASMER_TRAP_FUNCTION_OFFSETS_SECTION_NAME => {
                    trap_function_offsets = section.data().ok();
                }
                WASMER_TRAPS_SECTION_NAME => {
                    trap_section_data = section.data().ok();
                }
                _ => {}
            }
        }
        let trap_function_offsets = trap_function_offsets?;
        let trap_section_data = trap_section_data?;

        let offset_bytes = trap_function_offsets
            .get(local_index.index() * size_of::<usize>()..)
            .and_then(|s| s.get(..size_of::<usize>()))?;
        let trap_offset = usize::from_le_bytes(offset_bytes.try_into().unwrap());

        let traps = Self::parse_function_traps(trap_section_data, trap_offset, local_index).ok()?;
        let idx = traps
            .binary_search_by_key(&rel_pos, |info| info.code_offset)
            .ok()?;
        Some(traps[idx])
    }

    fn parse_function_traps(
        traps_section: &[u8],
        trap_offset: usize,
        local_function_index: LocalFunctionIndex,
    ) -> Result<Vec<TrapInformation>, String> {
        const WORD_SIZE: usize = size_of::<u32>();

        let data = traps_section
            .get(trap_offset..)
            .ok_or_else(|| "trap information points outside section data".to_string())?;
        let count_bytes = data.get(..WORD_SIZE).ok_or_else(|| {
            format!(
                "trap information for function {} is missing its trap count",
                local_function_index.index()
            )
        })?;
        let count = u32::from_le_bytes(
            count_bytes
                .try_into()
                .map_err(|e| format!("too many traps: {e}"))?,
        ) as usize;
        let data = &data[WORD_SIZE..];
        let records = data.get(..count * 2 * WORD_SIZE).ok_or_else(|| {
            format!(
                "trap information for function {} is truncated",
                local_function_index.index()
            )
        })?;

        let traps = records
            .chunks_exact(2 * WORD_SIZE)
            .map(|record| {
                let code_offset =
                    u32::from_le_bytes(record[..WORD_SIZE].try_into().map_err(|e| format!("{e}"))?);
                let trap_code =
                    u32::from_le_bytes(record[WORD_SIZE..].try_into().map_err(|e| format!("{e}"))?);
                // SAFETY: the serialized value is a TrapCode enum value
                let trap_code = unsafe { std::mem::transmute::<u32, TrapCode>(trap_code) };
                Ok(TrapInformation {
                    code_offset,
                    trap_code,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(traps)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[repr(transparent)]
/// A unique identifier for an Artifact.
pub struct ArtifactId {
    id: usize,
}

impl ArtifactId {
    /// Format this identifier as a string.
    pub fn id(&self) -> String {
        format!("{}", self.id)
    }
}

impl Clone for ArtifactId {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Default for ArtifactId {
    fn default() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, SeqCst),
        }
    }
}

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Artifact {
    id: ArtifactId,
    serializable: SerializableModule,
    #[cfg_attr(feature = "artifact-size", loupe(skip))]
    module_file: ModuleFile,
    // The artifact will only be allocated in memory in case we can execute it
    // (that means, if the target != host then this will be None).
    allocated: Option<AllocatedArtifact>,
}

impl Artifact {
    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &Engine,
        data: &[u8],
        tunables: &dyn Tunables,
        progress_callback: Option<CompilationProgressCallback>,
    ) -> Result<Self, CompileError> {
        let mut inner_engine = engine.inner_mut();
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;
        let module = translation.module;
        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let artifact = ArtifactBuild::new(
            &mut inner_engine,
            data,
            engine.target(),
            memory_styles,
            table_styles,
            progress_callback.as_ref(),
        )?;

        Self::from_parts(&mut inner_engine, artifact, engine.target()).map_err(|e| match e {
            DeserializeError::Compiler(c) => c,

            // `from_parts` only ever returns `CompileError`s when an
            // `ArtifactBuildVariant::Plain` is passed in. Other cases
            // of `DeserializeError` can only happen when an
            // `ArtifactBuildVariant::Archived` is passed in. We don't
            // wish to change the return type of this method because
            // a. it makes no sense and b. it would be a breaking change,
            // hence this match block and the other cases being
            // unreachable.
            _ => unreachable!(),
        })
    }

    /// This indicates if the Artifact is allocated and can be run by the current
    /// host. In case it can't be run (for example, if the artifact is cross compiled to
    /// other architecture), it will return false.
    pub fn allocated(&self) -> bool {
        self.allocated.is_some()
    }

    /// A unique identifier for this object.
    ///
    /// This exists to allow us to compare two Artifacts for equality. Otherwise,
    /// comparing two trait objects unsafely relies on implementation details
    /// of trait representation.
    pub fn id(&self) -> &ArtifactId {
        &self.id
    }

    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &Engine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Serialize the artifact.
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut reader = self.module_file.try_clone_reader().map_err(|e| {
            SerializeError::Generic(format!("Failed to serialize the Artifact: {e}"))
        })?;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map_err(|e| {
            SerializeError::Generic(format!("Failed to serialize the Artifact: {e}"))
        })?;
        Ok(buf)
    }

    /// Serialize the artifact to a file by copying the underlying binary file.
    ///
    /// The resulting file can later be loaded with [`Self::load_from_file`].
    pub fn serialize_to_file(&self, path: &Path) -> Result<(), SerializeError> {
        let mut reader = self.module_file.try_clone_reader().map_err(|e| {
            SerializeError::Generic(format!("Failed to serialize Artifact file: {e}"))
        })?;
        let mut writer = File::create(path).map_err(|e| {
            SerializeError::Generic(format!("Failed to serialize Artifact file: {e}"))
        })?;
        io::copy(&mut reader, &mut writer).map_err(|e| {
            SerializeError::Generic(format!("Failed to serialize Artifact file: {e}"))
        })?;
        Ok(())
    }

    /// Load a compiled artifact from a file.
    pub fn load_from_file(engine: &Engine, mut file: File) -> Result<Self, DeserializeError> {
        file.seek(SeekFrom::Start(0))
            .map_err(|e| DeserializeError::Generic(format!("cannot seek artifact format: {e}")))?;
        let reader = BufReader::new(
            file.try_clone()
                .map_err(|e| DeserializeError::Generic(e.to_string()))?,
        );
        let cache = ReadCache::new(reader);
        let image = object::File::parse(&cache)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("cannot parse image: {e}")))?;

        let module_info_section = image
            .sections()
            .find(|section| {
                section
                    .name_bytes()
                    .is_ok_and(|name| name == WASMER_MODULE_INFO_SECTION_NAME)
            })
            .ok_or_else(|| {
                DeserializeError::CorruptedBinary("missing ModuleInfo section".to_string())
            })?;

        let metadata_binary = module_info_section.data().map_err(|_| {
            DeserializeError::CorruptedBinary("cannot load ModuleInfo section data".to_string())
        })?;
        let serializable = SerializableModule::deserialize(metadata_binary)?;

        let artifact = ArtifactBuild {
            serializable,
            module_file: ModuleFile::OwnedFile(file),
        };
        let mut inner_engine = engine.inner_mut();
        Self::from_parts(&mut inner_engine, artifact, engine.target())
    }

    /// Construct a `ArtifactBuild` from component parts.
    fn from_parts(
        engine_inner: &mut EngineInner,
        mut artifact: ArtifactBuild,
        target: &Target,
    ) -> Result<Self, DeserializeError> {
        if !target.is_native() {
            todo!("remove the branch");
        } else {
            // check if cpu features are compatible before anything else
            let cpu_features = artifact.serializable.cpu_features();
            if !target.cpu_features().is_superset(cpu_features) {
                return Err(DeserializeError::Incompatible(format!(
                    "Some CPU Features needed for the artifact are missing: {:?}",
                    cpu_features.difference(*target.cpu_features())
                )));
            }
        }
        let module_info = artifact.serializable.module_info();
        let signatures = {
            let signature_registry = engine_inner.signatures();
            module_info
                .signatures
                .values()
                .zip(module_info.signature_hashes.values())
                .map(|(sig, sig_hash)| signature_registry.register(sig, *sig_hash))
                .collect::<PrimaryMap<_, _>>()
        };

        let allocated_artifact =
            AllocatedArtifact::from_binary(module_info, artifact.module_file.file(), signatures)
                // TODO
                .unwrap();
        let ArtifactBuild {
            module_file,
            serializable,
        } = artifact;
        let mut artifact = Self {
            id: Default::default(),
            module_file,
            serializable,
            allocated: Some(allocated_artifact),
        };

        artifact
            .internal_register_frame_info()
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{e:?}")))?;

        Ok(artifact)
    }

    /// Set module info
    pub fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.serializable.compile_info.module).is_some_and(|module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    /// Get ModuleInfo: TODO
    pub fn module_info(&self) -> &ModuleInfo {
        &self.serializable.compile_info.module
    }

    /// Return true if the beginning of file is a serialized Artifact.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(&object::elf::ELFMAG)
    }
}

impl PartialEq for Artifact {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Artifact {}

impl std::fmt::Debug for Artifact {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Artifact")
            .field("artifact_id", &self.id)
            .field("module_info", &self.serializable.module_info())
            .finish()
    }
}

impl Artifact {
    fn internal_register_frame_info(&mut self) -> Result<(), DeserializeError> {
        let module_info = self.serializable.compile_info.module.clone();
        let trap_file = self
            .module_file
            .file()
            .try_clone()
            .map_err(|e| DeserializeError::Generic(e.to_string()))?;
        let allocated = self.allocated.as_mut().expect("It must be allocated");
        if allocated.frame_info_registered {
            return Ok(());
        }

        let finished_function_extents = allocated.function_extents().into_boxed_slice();
        let trap_reader = Arc::new(TrapReader::new(trap_file));

        allocated.frame_info_registration = register_frame_info(
            module_info,
            &finished_function_extents,
            trap_reader,
            allocated._memory_map.base() as usize,
            allocated.debug_info.clone(),
        );

        allocated.frame_info_registered = true;

        Ok(())
    }

    /// Returns the functions allocated in memory or this `Artifact`
    /// ready to be run.
    pub fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_functions
    }

    /// Returns the start address and byte length of each locally-defined
    /// function body in this artifact.
    ///
    /// Returns `None` for cross-compiled artifacts (where the artifact has not
    /// been allocated into the host process).
    ///
    /// # Security
    ///
    /// The returned addresses are host-process pointers. They are not stable
    /// across runs and must not be forwarded to untrusted parties, as they
    /// reveal ASLR layout information.
    pub fn finished_function_extents(&self) -> Option<Vec<(LocalFunctionIndex, FunctionExtent)>> {
        let allocated = self.allocated.as_ref()?;
        Some(allocated.function_extents().into_iter().collect())
    }

    /// Return the maximum stack size used for each function (available only for the Singlepass compiler).
    pub fn finished_functions_max_stack_usage(
        &self,
    ) -> Option<Vec<(LocalFunctionIndex, Option<usize>)>> {
        self.allocated.as_ref()?;
        Some(
            self.serializable
                .compile_info
                .function_max_stack_usage
                .iter()
                .map(|(index, stack_usage)| (index, *stack_usage))
                .collect(),
        )
    }

    /// Returns the function call trampolines allocated in memory of this
    /// `Artifact`, ready to be run.
    pub fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_function_call_trampolines
    }

    /// Returns the dynamic function trampolines allocated in memory
    /// of this `Artifact`, ready to be run.
    pub fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .finished_dynamic_function_trampolines
    }

    /// Returns the associated VM signatures for this `Artifact`.
    pub fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSignatureHash> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .signatures
    }

    /// Crate an `Instance` from this `Artifact`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::new`].
    #[allow(clippy::result_large_err)]
    pub unsafe fn instantiate(
        &self,
        tunables: &dyn Tunables,
        imports: &[VMExtern],
        context: &mut StoreObjects,
    ) -> Result<VMInstance, InstantiationError> {
        unsafe {
            // Validate the CPU features this module was compiled with against the
            // host CPU features.
            let host_cpu_features = CpuFeature::for_host();
            if !host_cpu_features.is_superset(self.serializable.cpu_features()) {
                return Err(InstantiationError::CpuFeature(format!(
                    "{:?}",
                    self.serializable
                        .cpu_features()
                        .difference(host_cpu_features)
                )));
            }

            let module = self.serializable.compile_info.module.as_ref();

            let tags = resolve_tags(module, imports, context).map_err(InstantiationError::Link)?;

            let imports = resolve_imports(
                module,
                imports,
                context,
                self.finished_dynamic_function_trampolines(),
                self.serializable.memory_styles(),
                self.serializable.table_styles(),
            )
            .map_err(InstantiationError::Link)?;

            // Get pointers to where metadata about local memories should live in VM memory.
            // Get pointers to where metadata about local tables should live in VM memory.

            let cached_offsets = self
                .allocated
                .as_ref()
                .map(|a| a.vm_offsets.clone())
                .expect("Artifact::instantiate called on a non-host artifact");

            let (
                allocator,
                memory_definition_locations,
                table_definition_locations,
                global_definition_locations,
            ) = InstanceAllocator::new_with_offsets(cached_offsets, module);
            let finished_memories = tunables
                .create_memories(
                    context,
                    module,
                    self.serializable.memory_styles(),
                    &memory_definition_locations,
                )
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();
            let finished_tables = tunables
                .create_tables(
                    context,
                    module,
                    self.serializable.table_styles(),
                    &table_definition_locations,
                )
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();
            let finished_globals = tunables
                .create_globals(context, module, &global_definition_locations)
                .map_err(InstantiationError::Link)?
                .into_boxed_slice();

            let handle = VMInstance::new(
                allocator,
                self.serializable.compile_info.module.clone(),
                context,
                self.finished_functions().clone(),
                self.finished_function_call_trampolines().clone(),
                finished_memories,
                finished_tables,
                finished_globals,
                tags,
                imports,
                self.signatures().clone(),
            )
            .map_err(InstantiationError::Start)?;
            Ok(handle)
        }
    }

    /// Finishes the instantiation of a just created `VMInstance`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::finish_instantiation`].
    #[allow(clippy::result_large_err)]
    pub unsafe fn finish_instantiation(
        &self,
        config: &VMConfig,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        handle: &mut VMInstance,
    ) -> Result<(), InstantiationError> {
        unsafe {
            let data_initializers = self
                .serializable
                .data_initializers
                .iter()
                .map(|initializer: &OwnedDataInitializer| DataInitializer {
                    location: initializer.location.clone(),
                    data: &initializer.data,
                })
                .collect_vec();

            handle
                .finish_instantiation(config, trap_handler, &data_initializers)
                .map_err(InstantiationError::Start)
        }
    }

    #[allow(clippy::type_complexity)]
    #[cfg(feature = "static-artifact-create")]
    /// Generate a compilation
    pub fn generate_metadata<'data>(
        data: &'data [u8],
        compiler: &dyn Compiler,
        tunables: &dyn Tunables,
        features: &Features,
    ) -> Result<
        (
            CompileModuleInfo,
            PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
            Vec<DataInitializer<'data>>,
            Option<ModuleTranslationState>,
        ),
        CompileError,
    > {
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        // We try to apply the middleware first
        use crate::translator::ModuleMiddlewareChain;
        let mut module = translation.module;
        let middlewares = compiler.get_middlewares();
        middlewares
            .apply_on_module_info(&mut module)
            .map_err(|e| CompileError::MiddlewareError(e.to_string()))?;

        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features: features.clone(),
            memory_styles,
            table_styles,
            function_max_stack_usage: PrimaryMap::new(),
        };
        Ok((
            compile_info,
            translation.function_body_inputs,
            translation.data_initializers,
            translation.module_translation_state,
        ))
    }

    /// Generate the metadata object for the module
    #[cfg(feature = "static-artifact-create")]
    #[allow(clippy::type_complexity)]
    pub fn metadata<'a>(
        compiler: &dyn Compiler,
        data: &'a [u8],
        metadata_prefix: Option<&str>,
        target: &Target,
        tunables: &dyn Tunables,
        features: &Features,
    ) -> Result<
        (
            ModuleMetadata,
            Option<ModuleTranslationState>,
            PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
        ),
        CompileError,
    > {
        #[allow(dead_code)]
        let (compile_info, function_body_inputs, data_initializers, module_translation) =
            Self::generate_metadata(data, compiler, tunables, features)?;

        let data_initializers = data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        // TODO: we currently supply all-zero function body lengths.
        // We don't know the lengths until they're compiled, yet we have to
        // supply the metadata as an input to the compile.
        let function_body_lengths = function_body_inputs
            .keys()
            .map(|_function_body| 0u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let metadata = ModuleMetadata {
            compile_info,
            prefix: metadata_prefix.map(|s| s.to_string()).unwrap_or_default(),
            data_initializers,
            function_body_lengths,
            cpu_features: target.cpu_features().as_u64(),
        };

        Ok((metadata, module_translation, function_body_inputs))
    }

    /// Compile a module into an object file, which can be statically linked against.
    ///
    /// The `metadata_prefix` is an optional prefix for the object name to make the
    /// function names in the object file unique. When set, the function names will
    /// be `wasmer_function_{prefix}_{id}` and the object metadata will be addressable
    /// using `WASMER_METADATA_{prefix}_LENGTH` and `WASMER_METADATA_{prefix}_DATA`.
    ///
    #[cfg(feature = "static-artifact-create")]
    pub fn generate_object<'data>(
        _compiler: &dyn Compiler,
        _data: &[u8],
        _metadata_prefix: Option<&str>,
        _target: &'data Target,
        _tunables: &dyn Tunables,
        _features: &Features,
    ) -> Result<(ModuleInfo, object::write::Object<'data>, usize), CompileError> {
        todo!("used by a deprecated feature create_exe");
    }
}
