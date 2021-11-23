//! Define `DummyArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::DummyEngine;
use enumset::EnumSet;
use loupe::MemoryUsage;
#[cfg(feature = "serialize")]
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(feature = "compiler")]
use wasmer_compiler::ModuleEnvironment;
use wasmer_compiler::{CompileError, CpuFeature};
use wasmer_engine::{Artifact, DeserializeError, Engine as _, SerializeError, Tunables};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{
    Features, FunctionIndex, LocalFunctionIndex, MemoryIndex, ModuleInfo, OwnedDataInitializer,
    SignatureIndex, TableIndex,
};
use wasmer_vm::{
    FuncDataRegistry, FunctionBodyPtr, MemoryStyle, TableStyle, VMContext, VMFunctionBody,
    VMSharedSignatureIndex, VMTrampoline,
};

/// Serializable struct for the artifact
#[cfg_attr(feature = "serialize", derive(Serialize, Deserialize))]
#[derive(MemoryUsage)]
pub struct DummyArtifactMetadata {
    pub module: Arc<ModuleInfo>,
    pub features: Features,
    pub data_initializers: Box<[OwnedDataInitializer]>,
    // Plans for that module
    pub memory_styles: PrimaryMap<MemoryIndex, MemoryStyle>,
    pub table_styles: PrimaryMap<TableIndex, TableStyle>,
    pub cpu_features: u64,
}

/// A Dummy artifact.
///
/// This artifact will point to fake finished functions and trampolines
/// as no functions are really compiled.
#[derive(MemoryUsage)]
pub struct DummyArtifact {
    metadata: DummyArtifactMetadata,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    #[loupe(skip)]
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    func_data_registry: Arc<FuncDataRegistry>,
}

extern "C" fn dummy_function(_context: *mut VMContext) {
    panic!("Dummy engine can't generate functions")
}

extern "C" fn dummy_trampoline(
    _context: *mut VMContext,
    _callee: *const VMFunctionBody,
    _values: *mut u128,
) {
    panic!("Dummy engine can't generate trampolines")
}

impl DummyArtifact {
    const MAGIC_HEADER: &'static [u8] = b"\0wasmer-dummy";

    /// Check if the provided bytes look like a serialized `DummyArtifact`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(Self::MAGIC_HEADER)
    }

    #[cfg(feature = "compiler")]
    /// Compile a data buffer into a `DummyArtifact`, which may then be instantiated.
    pub fn new(
        engine: &DummyEngine,
        data: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();

        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = translation
            .module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = translation
            .module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let metadata = DummyArtifactMetadata {
            module: Arc::new(translation.module),
            features: Features::default(),
            data_initializers,
            memory_styles,
            table_styles,
            cpu_features: engine.target().cpu_features().as_u64(),
        };
        Self::from_parts(&engine, metadata)
    }

    #[cfg(not(feature = "compiler"))]
    pub fn new(engine: &DummyEngine, data: &[u8]) -> Result<Self, CompileError> {
        CompileError::Generic("The compiler feature is not enabled in the DummyEngine")
    }

    #[cfg(feature = "serialize")]
    /// Deserialize a DummyArtifact
    pub fn deserialize(engine: &DummyEngine, bytes: &[u8]) -> Result<Self, DeserializeError> {
        if !Self::is_deserializable(bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not of the dummy engine".to_string(),
            ));
        }

        let inner_bytes = &bytes[Self::MAGIC_HEADER.len()..];

        let metadata: DummyArtifactMetadata = bincode::deserialize(inner_bytes)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        Self::from_parts(&engine, metadata).map_err(DeserializeError::Compiler)
    }

    #[cfg(not(feature = "serialize"))]
    pub fn deserialize(engine: &DummyEngine, bytes: &[u8]) -> Result<Self, DeserializeError> {
        Err(DeserializeError::Generic(
            "The serializer feature is not enabled in the DummyEngine",
        ))
    }

    /// Construct a `DummyArtifact` from component parts.
    pub fn from_parts(
        engine: &DummyEngine,
        metadata: DummyArtifactMetadata,
    ) -> Result<Self, CompileError> {
        let num_local_functions =
            metadata.module.functions.len() - metadata.module.num_imported_functions;
        // We prepare the pointers for the finished functions.
        let finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> = (0
            ..num_local_functions)
            .map(|_| FunctionBodyPtr(dummy_function as _))
            .collect::<PrimaryMap<_, _>>();

        // We prepare the pointers for the finished function call trampolines.
        let finished_function_call_trampolines: PrimaryMap<SignatureIndex, VMTrampoline> = (0
            ..metadata.module.signatures.len())
            .map(|_| dummy_trampoline as VMTrampoline)
            .collect::<PrimaryMap<_, _>>();

        // We prepare the pointers for the finished dynamic function trampolines.
        let finished_dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBodyPtr> = (0
            ..metadata.module.num_imported_functions)
            .map(|_| FunctionBodyPtr(dummy_function as _))
            .collect::<PrimaryMap<_, _>>();

        // Compute indices into the shared signature table.
        let signatures = {
            metadata
                .module
                .signatures
                .values()
                .map(|sig| engine.register_signature(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        let finished_functions = finished_functions.into_boxed_slice();
        let finished_function_call_trampolines =
            finished_function_call_trampolines.into_boxed_slice();
        let finished_dynamic_function_trampolines =
            finished_dynamic_function_trampolines.into_boxed_slice();
        let signatures = signatures.into_boxed_slice();

        Ok(Self {
            metadata,
            finished_functions,
            finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
            signatures,
            func_data_registry: engine.func_data().clone(),
        })
    }
}

impl Artifact for DummyArtifact {
    fn module(&self) -> Arc<ModuleInfo> {
        self.metadata.module.clone()
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.metadata.module
    }

    fn module_mut(&mut self) -> Option<&mut ModuleInfo> {
        Arc::get_mut(&mut self.metadata.module)
    }

    fn register_frame_info(&self) {
        // Do nothing, since functions are not generated for the dummy engine
    }

    fn features(&self) -> &Features {
        &self.metadata.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.metadata.cpu_features)
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &*self.metadata.data_initializers
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.metadata.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.metadata.table_styles
    }

    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr> {
        &self.finished_functions
    }

    fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline> {
        &self.finished_function_call_trampolines
    }

    fn finished_dynamic_function_trampolines(&self) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    fn func_data_registry(&self) -> &FuncDataRegistry {
        &self.func_data_registry
    }

    #[cfg(feature = "serialize")]
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let bytes = bincode::serialize(&self.metadata)
            .map_err(|e| SerializeError::Generic(format!("{:?}", e)))?;

        // Prepend the header.
        let mut serialized = Self::MAGIC_HEADER.to_vec();
        serialized.extend(bytes);
        Ok(serialized)
    }

    #[cfg(not(feature = "serialize"))]
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Err(SerializeError::Generic(
            "The serializer feature is not enabled in the DummyEngine",
        ))
    }
}
