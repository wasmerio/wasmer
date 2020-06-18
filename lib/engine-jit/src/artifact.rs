//! Define `JITArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{JITEngine, JITEngineInner};
use crate::link::link_module;
#[cfg(feature = "compiler")]
use crate::serialize::SerializableCompilation;
use crate::serialize::SerializableModule;
use crate::unwind::UnwindRegistry;
use std::sync::{Arc, Mutex};
use wasm_common::entity::{BoxedSlice, PrimaryMap};
use wasm_common::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
#[cfg(feature = "compiler")]
use wasmer_compiler::ModuleEnvironment;
use wasmer_compiler::{CompileError, Features, Triple};
use wasmer_engine::{
    register_frame_info, Artifact, DeserializeError, GlobalFrameInfoRegistration, SerializeError,
};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, SerializableFunctionFrameInfo};
use wasmer_runtime::{MemoryPlan, ModuleInfo, TablePlan, VMFunctionBody, VMSharedSignatureIndex};

/// A compiled wasm module, ready to be instantiated.
pub struct JITArtifact {
    _unwind_registry: Arc<UnwindRegistry>,
    serializable: SerializableModule,
    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, *mut [VMFunctionBody]>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    frame_info_registration: Mutex<Option<GlobalFrameInfoRegistration>>,
}

impl JITArtifact {
    const MAGIC_HEADER: &'static [u8] = b"\0wasmer-jit";

    /// Check if the provided bytes look like a serialized `JITArtifact`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(Self::MAGIC_HEADER)
    }

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
            .values()
            .map(|memory_type| tunables.memory_plan(*memory_type))
            .collect();
        let table_plans: PrimaryMap<TableIndex, TablePlan> = translation
            .module
            .tables
            .values()
            .map(|table_type| tunables.table_plan(*table_type))
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
        let function_call_trampolines = compilation.get_function_call_trampolines();
        let dynamic_function_trampolines = compilation.get_dynamic_function_trampolines();

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
            debug: compilation.get_debug(),
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
    pub fn new(_jit: &JITEngine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a JITArtifact
    pub fn deserialize(jit: &JITEngine, bytes: &[u8]) -> Result<Self, DeserializeError> {
        if !Self::is_deserializable(bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not wasmer-jit".to_string(),
            ));
        }

        let inner_bytes = &bytes[Self::MAGIC_HEADER.len()..];

        // let r = flexbuffers::Reader::get_root(bytes).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        // let serializable = SerializableModule::deserialize(r).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        let serializable: SerializableModule = bincode::deserialize(inner_bytes)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        Self::from_parts(&mut jit.inner_mut(), serializable).map_err(DeserializeError::Compiler)
    }

    /// Construct a `JITArtifact` from component parts.
    pub fn from_parts(
        inner_jit: &mut JITEngineInner,
        serializable: SerializableModule,
    ) -> Result<Self, CompileError> {
        let mut unwind_registry = UnwindRegistry::new();
        let (
            finished_functions,
            _finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
        ) = inner_jit.allocate(
            &mut unwind_registry,
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

        let eh_frame = match &serializable.compilation.debug {
            Some(debug) => {
                let eh_frame_section_size = serializable.compilation.custom_sections
                    [debug.eh_frame]
                    .bytes
                    .len();
                let eh_frame_section_pointer = custom_sections[debug.eh_frame];
                Some(unsafe {
                    std::slice::from_raw_parts(eh_frame_section_pointer, eh_frame_section_size)
                })
            }
            None => None,
        };
        // Make all code compiled thus far executable.
        inner_jit.publish_compiled_code();

        unwind_registry.publish(eh_frame).map_err(|e| {
            CompileError::Resource(format!("Error while publishing the unwind code: {}", e))
        })?;

        let unwind_registry = Arc::new(unwind_registry);
        // Save the unwind registry into CodeMemory, so it can survive longer than
        // the Module.
        inner_jit.publish_unwind_registry(unwind_registry.clone());

        let finished_functions = finished_functions.into_boxed_slice();
        let finished_dynamic_function_trampolines =
            finished_dynamic_function_trampolines.into_boxed_slice();
        let signatures = signatures.into_boxed_slice();

        Ok(Self {
            _unwind_registry: unwind_registry,
            serializable,
            finished_functions,
            finished_dynamic_function_trampolines,
            signatures,
            frame_info_registration: Mutex::new(None),
        })
    }

    /// Get the default extension when serializing this artifact
    pub fn get_default_extension(_triple: &Triple) -> &'static str {
        // `.wjit` is the default extension for all the triples
        "wjit"
    }
}

impl Artifact for JITArtifact {
    fn module(&self) -> Arc<ModuleInfo> {
        self.serializable.module.clone()
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.serializable.module
    }

    fn module_mut(&mut self) -> Option<&mut ModuleInfo> {
        Arc::get_mut(&mut self.serializable.module)
    }

    fn register_frame_info(&self) {
        let mut info = self.frame_info_registration.lock().unwrap();

        if info.is_some() {
            return;
        }

        let frame_infos = &self.serializable.compilation.function_frame_info;
        let finished_functions = &self.finished_functions;
        *info = register_frame_info(
            self.serializable.module.clone(),
            finished_functions,
            frame_infos.clone(),
        );
    }

    fn features(&self) -> &Features {
        &self.serializable.features
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &*self.serializable.data_initializers
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.serializable.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.serializable.table_plans
    }

    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]> {
        &self.finished_functions
    }

    // TODO: return *const instead of *mut
    fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, *mut [VMFunctionBody]> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        // let mut s = flexbuffers::FlexbufferSerializer::new();
        // self.serializable.serialize(&mut s).map_err(|e| SerializeError::Generic(format!("{:?}", e)));
        // Ok(s.take_buffer())
        let bytes = bincode::serialize(&self.serializable)
            .map_err(|e| SerializeError::Generic(format!("{:?}", e)))?;

        // Prepend the header.
        let mut serialized = Self::MAGIC_HEADER.to_vec();
        serialized.extend(bytes);
        Ok(serialized)
    }
}
