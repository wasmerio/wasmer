//! Define `Artifact`, based on `ArtifactBuild`
//! to allow compiling and instantiating to be done as separate steps.

use crate::engine::link::link_module;
use crate::ArtifactBuild;
use crate::ArtifactCreate;
use crate::Features;
#[cfg(feature = "compiler")]
use crate::ModuleEnvironment;
use crate::{
    register_frame_info, resolve_imports, FunctionExtent, GlobalFrameInfoRegistration,
    InstantiationError, RuntimeError, Tunables,
};
use crate::{Engine, EngineInner};
use enumset::EnumSet;
use std::sync::Arc;
use std::sync::Mutex;
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::MetadataHeader;
use wasmer_types::{
    CompileError, CpuFeature, DataInitializer, DeserializeError, FunctionIndex, LocalFunctionIndex,
    MemoryIndex, ModuleInfo, OwnedDataInitializer, SerializableModule, SerializeError,
    SignatureIndex, TableIndex,
};
use wasmer_vm::{FunctionBodyPtr, MemoryStyle, TableStyle, VMSharedSignatureIndex, VMTrampoline};
use wasmer_vm::{InstanceAllocator, InstanceHandle, StoreObjects, TrapHandlerFn, VMExtern};

/// A compiled wasm module, ready to be instantiated.
pub struct Artifact {
    artifact: ArtifactBuild,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    frame_info_registration: Mutex<Option<GlobalFrameInfoRegistration>>,
    finished_function_lengths: BoxedSlice<LocalFunctionIndex, usize>,
}

impl Artifact {
    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &Engine,
        data: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let mut inner_engine = engine.inner_mut();
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

        Self::from_parts(&mut inner_engine, artifact)
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
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not wasmer-universal".to_string(),
            ));
        }
        let bytes = &bytes[ArtifactBuild::MAGIC_HEADER.len()..];
        let metadata_len = MetadataHeader::parse(bytes)?;
        let metadata_slice: &[u8] = &bytes[MetadataHeader::LEN..][..metadata_len];
        let serializable = SerializableModule::deserialize(metadata_slice)?;
        let artifact = ArtifactBuild::from_serializable(serializable);
        let mut inner_engine = engine.inner_mut();
        Self::from_parts(&mut inner_engine, artifact).map_err(DeserializeError::Compiler)
    }

    /// Construct a `ArtifactBuild` from component parts.
    pub fn from_parts(
        engine_inner: &mut EngineInner,
        artifact: ArtifactBuild,
    ) -> Result<Self, CompileError> {
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
            finished_functions,
            finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
            signatures,
            frame_info_registration: Mutex::new(None),
            finished_function_lengths,
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
        let mut info = self.frame_info_registration.lock().unwrap();

        if info.is_some() {
            return;
        }

        let finished_function_extents = self
            .finished_functions
            .values()
            .copied()
            .zip(self.finished_function_lengths.values().copied())
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

    /// Returns the functions allocated in memory or this `Artifact`
    /// ready to be run.
    pub fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr> {
        &self.finished_functions
    }

    /// Returns the function call trampolines allocated in memory of this
    /// `Artifact`, ready to be run.
    pub fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline> {
        &self.finished_function_call_trampolines
    }

    /// Returns the dynamic function trampolines allocated in memory
    /// of this `Artifact`, ready to be run.
    pub fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self.finished_dynamic_function_trampolines
    }

    /// Returns the associated VM signatures for this `Artifact`.
    pub fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    /// Do preinstantiation logic that is executed before instantiating
    pub fn preinstantiate(&self) -> Result<(), InstantiationError> {
        Ok(())
    }

    /// Crate an `Instance` from this `Artifact`.
    ///
    /// # Safety
    ///
    /// See [`InstanceHandle::new`].
    pub unsafe fn instantiate(
        &self,
        tunables: &dyn Tunables,
        imports: &[VMExtern],
        context: &mut StoreObjects,
    ) -> Result<InstanceHandle, InstantiationError> {
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
            InstanceAllocator::new(&*module);
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

        let handle = InstanceHandle::new(
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

    /// Finishes the instantiation of a just created `InstanceHandle`.
    ///
    /// # Safety
    ///
    /// See [`InstanceHandle::finish_instantiation`].
    pub unsafe fn finish_instantiation(
        &self,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        handle: &mut InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let data_initializers = self
            .data_initializers()
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>();
        handle
            .finish_instantiation(trap_handler, &data_initializers)
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }
}
