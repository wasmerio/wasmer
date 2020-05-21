//! Dummy Engine.

use crate::DummyArtifact;
use std::sync::Arc;
use wasm_common::FunctionType;
use wasmer_compiler::CompileError;
#[cfg(feature = "compiler")]
use wasmer_compiler::{Compiler, CompilerConfig};
use wasmer_engine::{
    Artifact, DeserializeError, Engine, InstantiationError, Resolver, RuntimeError, SerializeError,
    Tunables,
};
use wasmer_runtime::{
    InstanceHandle, SignatureRegistry, VMContext, VMFunctionBody, VMSharedSignatureIndex,
    VMTrampoline,
};

extern "C" fn dummy_trampoline(
    _context: *mut VMContext,
    _body: *const VMFunctionBody,
    _values: *mut u128,
) {
    panic!("Dummy engine can't call functions")
}

/// A WebAssembly `Dummy` Engine.
#[derive(Clone)]
pub struct DummyEngine {
    signatures: Arc<SignatureRegistry>,
}

impl DummyEngine {
    // A random dummy header
    #[allow(dead_code)]
    const DUMMY_HEADER: &'static [u8] = b"wasmer-dummy";

    #[cfg(feature = "compiler")]
    pub fn new() -> Self {
        Self {
            signatures: Arc::new(SignatureRegistry::new()),
        }
    }

    /// Check if the provided bytes look like a serialized
    /// module by the `Dumy` implementation.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(Self::DUMMY_HEADER)
    }
}

impl Engine for DummyEngine {
    /// Get the tunables
    fn tunables(&self) -> &dyn Tunables {
        unimplemented!("The dummy engine can't have tunables");
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
    fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        Some(dummy_trampoline)
    }

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        Err(CompileError::Codegen(
            "The dummy engine can't validate Wasm modules".to_string(),
        ))
    }

    /// Compile a WebAssembly binary
    fn compile(&self, binary: &[u8]) -> Result<Arc<dyn Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The dummy engine can't compile Wasm modules".to_string(),
        ))
    }

    /// Instantiates a WebAssembly module
    unsafe fn instantiate(
        &self,
        compiled_module: &dyn Artifact,
        resolver: &dyn Resolver,
    ) -> Result<InstanceHandle, InstantiationError> {
        Err(InstantiationError::Start(RuntimeError::new(
            "The dummy engine can't instantiate Wasm modules".to_string(),
        )))
    }

    /// Finish the instantiation of a WebAssembly module
    unsafe fn finish_instantiation(
        &self,
        compiled_module: &dyn Artifact,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        Err(InstantiationError::Start(RuntimeError::new(
            "The dummy engine can't instantiate Wasm modules".to_string(),
        )))
    }

    /// Serializes a WebAssembly module
    fn serialize(&self, compiled_module: &dyn Artifact) -> Result<Vec<u8>, SerializeError> {
        Err(SerializeError::Generic(
            "The dummy engine can't serialize Wasm modules".to_string(),
        ))
    }

    /// Deserializes a WebAssembly module (binary content of a Shared Object file)
    fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError> {
        if !Self::is_deserializable(&bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any native format Wasmer can understand".to_string(),
            ));
        }
        Err(DeserializeError::Generic(
            "The dummy engine can't deserialize Wasm modules".to_string(),
        ))
    }
}
