//! Define `Artifact`, based on `ArtifactBuild`
//! to allow compiling and instantiating to be done as separate steps.

use crate::engine::link::link_module;
use crate::ArtifactBuild;
use crate::ArtifactCreate;
use crate::Features;
use crate::ModuleEnvironment;
use crate::{
    register_frame_info, resolve_imports, FunctionExtent, GlobalFrameInfoRegistration,
    InstantiationError, RuntimeError, Tunables,
};
#[cfg(feature = "static-artifact-create")]
use crate::{Compiler, FunctionBodyData, ModuleTranslationState};
use crate::{Engine, EngineInner};
use enumset::EnumSet;
#[cfg(any(feature = "static-artifact-create", feature = "static-artifact-load"))]
use std::mem;
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(feature = "static-artifact-create")]
use wasmer_object::{emit_compilation, emit_data, get_object_for_target, Object};
#[cfg(any(feature = "static-artifact-create", feature = "static-artifact-load"))]
use wasmer_types::compilation::symbols::ModuleMetadata;
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
#[cfg(feature = "static-artifact-create")]
use wasmer_types::CompileModuleInfo;
use wasmer_types::MetadataHeader;
#[cfg(feature = "static-artifact-load")]
use wasmer_types::SerializableCompilation;
use wasmer_types::{
    CompileError, CpuFeature, DataInitializer, DeserializeError, FunctionIndex, LocalFunctionIndex,
    MemoryIndex, ModuleInfo, OwnedDataInitializer, SignatureIndex, TableIndex, Target,
};
use wasmer_types::{SerializableModule, SerializeError};
use wasmer_vm::{FunctionBodyPtr, MemoryStyle, TableStyle, VMSharedSignatureIndex, VMTrampoline};
use wasmer_vm::{InstanceAllocator, StoreObjects, TrapHandlerFn, VMExtern, VMInstance};

pub struct AllocatedArtifact {
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    /// Some(_) only if this is not a deserialized static artifact
    frame_info_registration: Option<Mutex<Option<GlobalFrameInfoRegistration>>>,
    finished_function_lengths: BoxedSlice<LocalFunctionIndex, usize>,
}

/// A compiled wasm module, ready to be instantiated.
pub struct Artifact {
    artifact: ArtifactBuild,
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
        )?;

        Self::from_parts(&mut inner_engine, artifact, engine.target())
    }

    /// This indicates if the Artifact is allocated and can be run by the current
    /// host. In case it can't be run (for example, if the artifact is cross compiled to
    /// other architecture), it will return false.
    pub fn allocated(&self) -> bool {
        self.allocated.is_some()
    }

    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &Engine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a ArtifactBuild
    ///
    /// # Safety
    /// This function is unsafe because rkyv reads directly without validating
    /// the data.
    pub unsafe fn deserialize(engine: &Engine, bytes: &[u8]) -> Result<Self, DeserializeError> {
        if !ArtifactBuild::is_deserializable(bytes) {
            let static_artifact = Self::deserialize_object(engine, bytes);
            match static_artifact {
                Ok(v) => {
                    return Ok(v);
                }
                Err(err) => {
                    eprintln!("Could not deserialize as static object: {}", err);
                }
            }
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not wasmer-universal".to_string(),
            ));
        }

        let bytes = Self::get_byte_slice(bytes, ArtifactBuild::MAGIC_HEADER.len(), bytes.len())?;

        let metadata_len = MetadataHeader::parse(bytes)?;
        let metadata_slice = Self::get_byte_slice(bytes, MetadataHeader::LEN, bytes.len())?;
        let metadata_slice = Self::get_byte_slice(metadata_slice, 0, metadata_len)?;

        let serializable = SerializableModule::deserialize(metadata_slice)?;
        let artifact = ArtifactBuild::from_serializable(serializable);
        let mut inner_engine = engine.inner_mut();
        Self::from_parts(&mut inner_engine, artifact, engine.target())
            .map_err(DeserializeError::Compiler)
    }

    /// Construct a `ArtifactBuild` from component parts.
    pub fn from_parts(
        engine_inner: &mut EngineInner,
        artifact: ArtifactBuild,
        target: &Target,
    ) -> Result<Self, CompileError> {
        if !target.is_native() {
            return Ok(Self {
                artifact,
                allocated: None,
            });
        }
        let module_info = artifact.create_module_info();
        let (
            finished_functions,
            finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
            custom_sections,
        ) = engine_inner.allocate(
            &module_info,
            artifact.get_function_bodies_ref(),
            artifact.get_function_call_trampolines_ref(),
            artifact.get_dynamic_function_trampolines_ref(),
            artifact.get_custom_sections_ref(),
        )?;

        link_module(
            &module_info,
            &finished_functions,
            artifact.get_function_relocations(),
            &custom_sections,
            artifact.get_custom_section_relocations_ref(),
            artifact.get_libcall_trampolines(),
            artifact.get_libcall_trampoline_len(),
        );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = engine_inner.signatures();
            module_info
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        let eh_frame = match artifact.get_debug_ref() {
            Some(debug) => {
                let eh_frame_section_size = artifact.get_custom_sections_ref()[debug.eh_frame]
                    .bytes
                    .len();
                let eh_frame_section_pointer = custom_sections[debug.eh_frame];
                Some(unsafe {
                    std::slice::from_raw_parts(*eh_frame_section_pointer, eh_frame_section_size)
                })
            }
            None => None,
        };

        // Make all code compiled thus far executable.
        engine_inner.publish_compiled_code();

        engine_inner.publish_eh_frame(eh_frame)?;

        let finished_function_lengths = finished_functions
            .values()
            .map(|extent| extent.length)
            .collect::<PrimaryMap<LocalFunctionIndex, usize>>()
            .into_boxed_slice();
        let finished_functions = finished_functions
            .values()
            .map(|extent| extent.ptr)
            .collect::<PrimaryMap<LocalFunctionIndex, FunctionBodyPtr>>()
            .into_boxed_slice();
        let finished_function_call_trampolines =
            finished_function_call_trampolines.into_boxed_slice();
        let finished_dynamic_function_trampolines =
            finished_dynamic_function_trampolines.into_boxed_slice();
        let signatures = signatures.into_boxed_slice();

        Ok(Self {
            artifact,
            allocated: Some(AllocatedArtifact {
                finished_functions,
                finished_function_call_trampolines,
                finished_dynamic_function_trampolines,
                signatures,
                frame_info_registration: Some(Mutex::new(None)),
                finished_function_lengths,
            }),
        })
    }

    /// Check if the provided bytes look like a serialized `ArtifactBuild`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        ArtifactBuild::is_deserializable(bytes)
    }
}

impl ArtifactCreate for Artifact {
    fn create_module_info(&self) -> ModuleInfo {
        self.artifact.create_module_info()
    }

    fn features(&self) -> &Features {
        self.artifact.features()
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        self.artifact.cpu_features()
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        self.artifact.data_initializers()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        self.artifact.memory_styles()
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        self.artifact.table_styles()
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        self.artifact.serialize()
    }
}

impl Artifact {
    /// Register thie `Artifact` stack frame information into the global scope.
    ///
    /// This is required to ensure that any traps can be properly symbolicated.
    pub fn register_frame_info(&self) {
        if let Some(frame_info_registration) = self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .frame_info_registration
            .as_ref()
        {
            let mut info = frame_info_registration.lock().unwrap();

            if info.is_some() {
                return;
            }

            let finished_function_extents = self
                .allocated
                .as_ref()
                .expect("It must be allocated")
                .finished_functions
                .values()
                .copied()
                .zip(
                    self.allocated
                        .as_ref()
                        .expect("It must be allocated")
                        .finished_function_lengths
                        .values()
                        .copied(),
                )
                .map(|(ptr, length)| FunctionExtent { ptr, length })
                .collect::<PrimaryMap<LocalFunctionIndex, _>>()
                .into_boxed_slice();

            let frame_infos = self.artifact.get_frame_info_ref();
            *info = register_frame_info(
                self.artifact.create_module_info(),
                &finished_function_extents,
                frame_infos.clone(),
            );
        }
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
    pub fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self
            .allocated
            .as_ref()
            .expect("It must be allocated")
            .signatures
    }

    /// Do preinstantiation logic that is executed before instantiating
    pub fn preinstantiate(&self) -> Result<(), InstantiationError> {
        Ok(())
    }

    /// Crate an `Instance` from this `Artifact`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::new`].
    pub unsafe fn instantiate(
        &self,
        tunables: &dyn Tunables,
        imports: &[VMExtern],
        context: &mut StoreObjects,
    ) -> Result<VMInstance, InstantiationError> {
        // Validate the CPU features this module was compiled with against the
        // host CPU features.
        let host_cpu_features = CpuFeature::for_host();
        if !host_cpu_features.is_superset(self.cpu_features()) {
            return Err(InstantiationError::CpuFeature(format!(
                "{:?}",
                self.cpu_features().difference(host_cpu_features)
            )));
        }

        self.preinstantiate()?;

        let module = Arc::new(self.create_module_info());
        let imports = resolve_imports(
            &module,
            imports,
            context,
            self.finished_dynamic_function_trampolines(),
            self.memory_styles(),
            self.table_styles(),
        )
        .map_err(InstantiationError::Link)?;

        // Get pointers to where metadata about local memories should live in VM memory.
        // Get pointers to where metadata about local tables should live in VM memory.

        let (allocator, memory_definition_locations, table_definition_locations) =
            InstanceAllocator::new(&module);
        let finished_memories = tunables
            .create_memories(
                context,
                &module,
                self.memory_styles(),
                &memory_definition_locations,
            )
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_tables = tunables
            .create_tables(
                context,
                &module,
                self.table_styles(),
                &table_definition_locations,
            )
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_globals = tunables
            .create_globals(context, &module)
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();

        self.register_frame_info();

        let handle = VMInstance::new(
            allocator,
            module,
            context,
            self.finished_functions().clone(),
            self.finished_function_call_trampolines().clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            self.signatures().clone(),
        )
        .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))?;
        Ok(handle)
    }

    /// Finishes the instantiation of a just created `VMInstance`.
    ///
    /// # Safety
    ///
    /// See [`VMInstance::finish_instantiation`].
    pub unsafe fn finish_instantiation(
        &self,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        handle: &mut VMInstance,
    ) -> Result<(), InstantiationError> {
        let data_initializers = self
            .data_initializers()
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &init.data,
            })
            .collect::<Vec<_>>();
        handle
            .finish_instantiation(trap_handler, &data_initializers)
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
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
        middlewares.apply_on_module_info(&mut module);

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
            module,
            features: features.clone(),
            memory_styles,
            table_styles,
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
    pub fn metadata<'data, 'a>(
        compiler: &dyn Compiler,
        data: &'a [u8],
        metadata_prefix: Option<&str>,
        target: &'data Target,
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
        compiler: &dyn Compiler,
        data: &[u8],
        metadata_prefix: Option<&str>,
        target: &'data Target,
        tunables: &dyn Tunables,
        features: &Features,
    ) -> Result<
        (
            ModuleInfo,
            Object<'data>,
            usize,
            Box<dyn wasmer_types::SymbolRegistry>,
        ),
        CompileError,
    > {
        use wasmer_types::{compilation::symbols::ModuleMetadataSymbolRegistry, SymbolRegistry};

        fn to_compile_error(err: impl std::error::Error) -> CompileError {
            CompileError::Codegen(format!("{}", err))
        }

        let target_triple = target.triple();
        let (mut metadata, module_translation, function_body_inputs) =
            Self::metadata(compiler, data, metadata_prefix, target, tunables, features)
                .map_err(to_compile_error)?;

        /*
        In the C file we need:
        - imports
        - exports

        to construct an api::Module which is a Store (can be passed in via argument) and an
        Arc<dyn Artifact> which means this struct which includes:
        - CompileModuleInfo
        - Features
        - ModuleInfo
        - MemoryIndex -> MemoryStyle
        - TableIndex -> TableStyle
        - LocalFunctionIndex -> FunctionBodyPtr // finished functions
        - FunctionIndex -> FunctionBodyPtr // finished dynamic function trampolines
        - SignatureIndex -> VMSharedSignatureindextureIndex // signatures
         */

        let serialized_data = metadata.serialize().map_err(to_compile_error)?;
        let mut metadata_binary = vec![];
        metadata_binary.extend(MetadataHeader::new(serialized_data.len()).into_bytes());
        metadata_binary.extend(serialized_data);

        let (_compile_info, symbol_registry) = metadata.split();

        let compilation: wasmer_types::compilation::function::Compilation = compiler
            .compile_module(
                target,
                &metadata.compile_info,
                module_translation.as_ref().unwrap(),
                function_body_inputs,
            )?;
        let mut obj = get_object_for_target(target_triple).map_err(to_compile_error)?;

        let object_name = ModuleMetadataSymbolRegistry {
            prefix: metadata_prefix.unwrap_or_default().to_string(),
        }
        .symbol_to_name(wasmer_types::Symbol::Metadata);

        emit_data(&mut obj, object_name.as_bytes(), &metadata_binary, 1)
            .map_err(to_compile_error)?;

        emit_compilation(&mut obj, compilation, &symbol_registry, target_triple)
            .map_err(to_compile_error)?;
        Ok((
            metadata.compile_info.module,
            obj,
            metadata_binary.len(),
            Box::new(symbol_registry),
        ))
    }

    /// Deserialize a ArtifactBuild from an object file
    ///
    /// # Safety
    /// The object must be a valid static object generated by wasmer.
    #[cfg(not(feature = "static-artifact-load"))]
    pub unsafe fn deserialize_object(
        _engine: &Engine,
        _bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        Err(DeserializeError::Compiler(
            CompileError::UnsupportedFeature("static load is not compiled in".to_string()),
        ))
    }

    fn get_byte_slice(input: &[u8], start: usize, end: usize) -> Result<&[u8], DeserializeError> {
        if (start == end && input.len() > start)
            || (start < end && input.len() > start && input.len() >= end)
        {
            Ok(&input[start..end])
        } else {
            Err(DeserializeError::InvalidByteLength {
                expected: end - start,
                got: input.len(),
            })
        }
    }

    /// Deserialize a ArtifactBuild from an object file
    ///
    /// # Safety
    /// The object must be a valid static object generated by wasmer.
    #[cfg(feature = "static-artifact-load")]
    pub unsafe fn deserialize_object(
        engine: &Engine,
        bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        let metadata_len = MetadataHeader::parse(bytes)?;
        let metadata_slice = Self::get_byte_slice(bytes, MetadataHeader::LEN, bytes.len())?;
        let metadata_slice = Self::get_byte_slice(metadata_slice, 0, metadata_len)?;
        let metadata: ModuleMetadata = ModuleMetadata::deserialize(metadata_slice)?;

        const WORD_SIZE: usize = mem::size_of::<usize>();
        let mut byte_buffer = [0u8; WORD_SIZE];

        let mut cur_offset = MetadataHeader::LEN + metadata_len;

        let byte_buffer_slice = Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
        byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
        cur_offset += WORD_SIZE;

        let num_finished_functions = usize::from_ne_bytes(byte_buffer);
        let mut finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> =
            PrimaryMap::new();

        let engine_inner = engine.inner();
        let signature_registry = engine_inner.signatures();

        // read finished functions in order now...
        for _i in 0..num_finished_functions {
            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
            cur_offset += WORD_SIZE;

            // TODO: we can read back the length here if we serialize it. This will improve debug output.
            finished_functions.push(fp);
        }

        // We register all the signatures
        let signatures = {
            metadata
                .compile_info
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        // read trampolines in order
        let mut finished_function_call_trampolines = PrimaryMap::new();

        let byte_buffer_slice = Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
        byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
        cur_offset += WORD_SIZE;
        let num_function_trampolines = usize::from_ne_bytes(byte_buffer);
        for _ in 0..num_function_trampolines {
            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            cur_offset += WORD_SIZE;
            let trampoline_ptr_bytes = usize::from_ne_bytes(byte_buffer);
            let trampoline = mem::transmute::<usize, VMTrampoline>(trampoline_ptr_bytes);
            finished_function_call_trampolines.push(trampoline);
            // TODO: we can read back the length here if we serialize it. This will improve debug output.
        }

        // read dynamic function trampolines in order now...
        let mut finished_dynamic_function_trampolines = PrimaryMap::new();
        let byte_buffer_slice = Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
        byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
        cur_offset += WORD_SIZE;
        let num_dynamic_trampoline_functions = usize::from_ne_bytes(byte_buffer);
        for _i in 0..num_dynamic_trampoline_functions {
            let byte_buffer_slice =
                Self::get_byte_slice(bytes, cur_offset, cur_offset + WORD_SIZE)?;
            byte_buffer[0..WORD_SIZE].clone_from_slice(byte_buffer_slice);
            let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
            cur_offset += WORD_SIZE;

            // TODO: we can read back the length here if we serialize it. This will improve debug output.

            finished_dynamic_function_trampolines.push(fp);
        }

        let artifact = ArtifactBuild::from_serializable(SerializableModule {
            compilation: SerializableCompilation::default(),
            compile_info: metadata.compile_info,
            data_initializers: metadata.data_initializers,
            cpu_features: metadata.cpu_features,
        });

        let finished_function_lengths = finished_functions
            .values()
            .map(|_| 0)
            .collect::<PrimaryMap<LocalFunctionIndex, usize>>()
            .into_boxed_slice();

        Ok(Self {
            artifact,
            allocated: Some(AllocatedArtifact {
                finished_functions: finished_functions.into_boxed_slice(),
                finished_function_call_trampolines: finished_function_call_trampolines
                    .into_boxed_slice(),
                finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                    .into_boxed_slice(),
                signatures: signatures.into_boxed_slice(),
                finished_function_lengths,
                frame_info_registration: None,
            }),
        })
    }
}
