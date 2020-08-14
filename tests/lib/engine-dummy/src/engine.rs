//! Dummy Engine.

use crate::DummyArtifact;
use std::sync::Arc;
use wasmer_compiler::{CompileError, Features, Target};
use wasmer_engine::{Artifact, DeserializeError, Engine, EngineId, Tunables};
use wasmer_types::FunctionType;
use wasmer_vm::{
    SignatureRegistry, VMContext, VMFunctionBody, VMSharedSignatureIndex, VMTrampoline,
};

extern "C" fn dummy_trampoline(
    _context: *mut VMContext,
    _body: *const VMFunctionBody,
    _values: *mut u128,
) {
    panic!("Dummy engine can't call functions, since Wasm function bodies are not really compiled")
}

/// A WebAssembly `Dummy` Engine.
#[derive(Clone)]
pub struct DummyEngine {
    signatures: Arc<SignatureRegistry>,
    features: Arc<Features>,
    target: Arc<Target>,
    engine_id: EngineId,
}

impl DummyEngine {
    #[cfg(feature = "compiler")]
    pub fn new() -> Self {
        Self {
            signatures: Arc::new(SignatureRegistry::new()),
            features: Arc::new(Default::default()),
            target: Arc::new(Default::default()),
            engine_id: EngineId::default(),
        }
    }

    pub fn features(&self) -> &Features {
        &self.features
    }
}

impl Engine for DummyEngine {
    /// Get the tunables
    fn target(&self) -> &Target {
        &self.target
    }

    /// Register a signature
    fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex {
        self.signatures.register(func_type)
    }

    /// Lookup a signature
    fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType> {
        self.signatures.lookup(sig)
    }

    /// Retrieves a trampoline given a signature
    fn function_call_trampoline(&self, _sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        Some(dummy_trampoline)
    }

    #[cfg(feature = "compiler")]
    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        use wasmer_compiler::wasmparser::{
            validate, OperatorValidatorConfig, ValidatingParserConfig,
        };

        let features = self.features();
        let config = ValidatingParserConfig {
            operator_config: OperatorValidatorConfig {
                enable_threads: features.threads,
                enable_reference_types: features.reference_types,
                enable_bulk_memory: features.bulk_memory,
                enable_simd: features.simd,
                enable_tail_call: false,
                enable_multi_value: features.multi_value,
            },
        };
        validate(binary, Some(config)).map_err(|e| CompileError::Validate(format!("{}", e)))
    }

    #[cfg(not(feature = "compiler"))]
    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        // We mark all Wasm modules as valid
        Ok(())
    }

    /// Compile a WebAssembly binary
    fn compile(
        &self,
        binary: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Arc<dyn Artifact>, CompileError> {
        Ok(Arc::new(DummyArtifact::new(&self, binary, tunables)?))
    }

    /// Deserializes a WebAssembly module (binary content of a Shared Object file)
    unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError> {
        Ok(Arc::new(DummyArtifact::deserialize(&self, &bytes)?))
    }

    fn id(&self) -> &EngineId {
        &self.engine_id
    }

    fn cloned(&self) -> Arc<dyn Engine + Send + Sync> {
        Arc::new(self.clone())
    }
}
