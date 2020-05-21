//! Define `JITArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{JITEngine, JITEngineInner};
use crate::link::link_module;
use crate::serialize::{SerializableCompilation, SerializableModule};
use std::any::Any;
use std::sync::{Arc, Mutex};
use wasm_common::entity::{BoxedSlice, PrimaryMap};
use wasm_common::{
    DataInitializer, FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer,
    SignatureIndex, TableIndex,
};
use wasmer_compiler::CompileError;
#[cfg(feature = "compiler")]
use wasmer_compiler::ModuleEnvironment;
use wasmer_engine::{
    register_frame_info, resolve_imports, Artifact, DeserializeError, Engine,
    GlobalFrameInfoRegistration, InstantiationError, Resolver, RuntimeError,
    SerializableFunctionFrameInfo, SerializeError, Tunables,
};
use wasmer_runtime::{
    InstanceHandle, ModuleInfo, SignatureRegistry, VMFunctionBody, VMSharedSignatureIndex,
};

use wasmer_runtime::{MemoryPlan, TablePlan};

/// A compiled wasm module, ready to be instantiated.
pub struct JITArtifact {
    serializable: SerializableModule,

    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, *const VMFunctionBody>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    frame_info_registration: Option<GlobalFrameInfoRegistration>,
}

impl JITArtifact {
    /// Compile a data buffer into a `JITArtifact`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(jit: &JITEngine, data: &[u8]) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let mut inner_jit = jit.inner_mut();
        let tunables = jit.tunables();

        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        let memory_plans: PrimaryMap<MemoryIndex, MemoryPlan> = translation
            .module
            .memories
            .iter()
            .map(|(_index, memory_type)| tunables.memory_plan(*memory_type))
            .collect();
        let table_plans: PrimaryMap<TableIndex, TablePlan> = translation
            .module
            .tables
            .iter()
            .map(|(_index, table_type)| tunables.table_plan(*table_type))
            .collect();

        let compiler = inner_jit.compiler()?;

        // Compile the Module
        let compilation = compiler.compile_module(
            &translation.module,
            translation.module_translation.as_ref().unwrap(),
            translation.function_body_inputs,
            memory_plans.clone(),
            table_plans.clone(),
        )?;

        // Compile the trampolines
        let func_types = translation
            .module
            .signatures
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let function_call_trampolines = compiler
            .compile_function_call_trampolines(&func_types)?
            .into_iter()
            .collect::<PrimaryMap<SignatureIndex, _>>();

        let dynamic_function_trampolines =
            compiler.compile_dynamic_function_trampolines(&translation.module)?;

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let frame_infos = compilation
            .get_frame_info()
            .values()
            .map(|frame_info| SerializableFunctionFrameInfo::Processed(frame_info.clone()))
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let serializable_compilation = SerializableCompilation {
            function_bodies: compilation.get_function_bodies(),
            function_relocations: compilation.get_relocations(),
            function_jt_offsets: compilation.get_jt_offsets(),
            function_frame_info: frame_infos,
            function_call_trampolines,
            dynamic_function_trampolines,
            custom_sections: compilation.get_custom_sections(),
            custom_section_relocations: compilation.get_custom_section_relocations(),
        };
        let serializable = SerializableModule {
            compilation: serializable_compilation,
            module: Arc::new(translation.module),
            features: inner_jit.compiler()?.features().clone(),
            data_initializers,
            memory_plans,
            table_plans,
        };
        Self::from_parts(&mut inner_jit, serializable)
    }

    /// Compile a data buffer into a `JITArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(jit: &JITEngine, data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Serialize a JITArtifact
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        // let mut s = flexbuffers::FlexbufferSerializer::new();
        // self.serializable.serialize(&mut s).map_err(|e| SerializeError::Generic(format!("{:?}", e)));
        // Ok(s.take_buffer())
        bincode::serialize(&self.serializable)
            .map_err(|e| SerializeError::Generic(format!("{:?}", e)))
    }

    /// Deserialize a JITArtifact
    pub fn deserialize(jit: &JITEngine, bytes: &[u8]) -> Result<Self, DeserializeError> {
        // let r = flexbuffers::Reader::get_root(bytes).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        // let serializable = SerializableModule::deserialize(r).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        let serializable: SerializableModule = bincode::deserialize(bytes)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        Self::from_parts(&mut jit.inner_mut(), serializable).map_err(DeserializeError::Compiler)
    }

    /// Construct a `JITArtifact` from component parts.
    pub fn from_parts(
        inner_jit: &mut JITEngineInner,
        serializable: SerializableModule,
    ) -> Result<Self, CompileError> {
        let (finished_functions, finished_dynamic_function_trampolines) = inner_jit.allocate(
            &serializable.module,
            &serializable.compilation.function_bodies,
            &serializable.compilation.function_call_trampolines,
            &serializable.compilation.dynamic_function_trampolines,
        )?;
        let custom_sections =
            inner_jit.allocate_custom_sections(&serializable.compilation.custom_sections)?;

        link_module(
            &serializable.module,
            &finished_functions,
            &serializable.compilation.function_jt_offsets,
            serializable.compilation.function_relocations.clone(),
            &custom_sections,
            &serializable.compilation.custom_section_relocations,
        );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = inner_jit.signatures();
            serializable
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        // Make all code compiled thus far executable.
        inner_jit.publish_compiled_code();

        let finished_functions = finished_functions.into_boxed_slice();
        let finished_dynamic_function_trampolines =
            finished_dynamic_function_trampolines.into_boxed_slice();
        let signatures = signatures.into_boxed_slice();
        let frame_info_registration = register_frame_info(
            serializable.module.clone(),
            &finished_functions,
            serializable.compilation.function_frame_info.clone(),
        );

        Ok(Self {
            serializable,
            finished_functions,
            finished_dynamic_function_trampolines,
            signatures,
            frame_info_registration,
        })
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.serializable.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.serializable.table_plans
    }

    /// Crate an `Instance` from this `JITArtifact`.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::new`
    pub unsafe fn instantiate(
        &self,
        tunables: &Tunables,
        sig_registry: &SignatureRegistry,
        resolver: &dyn Resolver,
        host_state: Box<dyn Any>,
    ) -> Result<InstanceHandle, InstantiationError> {
        let imports = resolve_imports(
            &self.module(),
            &sig_registry,
            resolver,
            &self.finished_dynamic_function_trampolines,
            self.memory_plans(),
            self.table_plans(),
        )
        .map_err(InstantiationError::Link)?;

        let finished_memories = tunables
            .create_memories(&self.module(), self.memory_plans())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_tables = tunables
            .create_tables(&self.module(), self.table_plans())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_globals = tunables
            .create_globals(&self.module())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();

        InstanceHandle::new(
            self.serializable.module.clone(),
            self.finished_functions.clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            self.signatures.clone(),
            host_state,
        )
        .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Finishes the instantiation of a just created `InstanceHandle`.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::finish_instantiation`
    pub unsafe fn finish_instantiation(
        &self,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let is_bulk_memory: bool = self.serializable.features.bulk_memory;
        handle
            .finish_instantiation(is_bulk_memory, &self.data_initializers())
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Returns data initializers to pass to `InstanceHandle::initialize`
    pub fn data_initializers(&self) -> Vec<DataInitializer<'_>> {
        self.serializable
            .data_initializers
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>()
    }
}

impl Artifact for JITArtifact {
    fn module(&self) -> &ModuleInfo {
        &self.serializable.module
    }

    fn module_mut(&mut self) -> &mut ModuleInfo {
        Arc::get_mut(&mut self.serializable.module).unwrap()
    }
}
