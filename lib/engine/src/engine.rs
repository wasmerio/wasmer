//! JIT compilation.

use crate::error::InstantiationError;
use crate::resolver::Resolver;
use crate::tunables::Tunables;
use crate::{CompiledModule, DeserializeError, SerializeError};
use std::sync::Arc;
use wasm_common::FunctionType;
use wasmer_compiler::CompileError;
use wasmer_runtime::{InstanceHandle, VMSharedSignatureIndex, VMTrampoline};

/// A unimplemented Wasmer `Engine`.
/// This trait is used by implementors to implement custom engines,
/// such as: JIT or Native.
pub trait Engine {
    /// Get the tunables
    fn tunables(&self) -> &Tunables;

    /// Register a signature
    fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex;

    /// Lookup a signature
    fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType>;

    /// Retrieves a trampoline given a signature
    fn trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline>;

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError>;

    /// Compile a WebAssembly binary
    fn compile(&self, binary: &[u8]) -> Result<Arc<CompiledModule>, CompileError>;

    /// Instantiates a WebAssembly module
    unsafe fn instantiate(
        &self,
        compiled_module: &dyn CompiledModule,
        resolver: &dyn Resolver,
    ) -> Result<InstanceHandle, InstantiationError>;

    /// Serializes a WebAssembly module
    fn serialize(&self, compiled_module: &dyn CompiledModule) -> Result<Vec<u8>, SerializeError>;

    /// Deserializes a WebAssembly module
    fn deserialize(&self, bytes: &[u8]) -> Result<Arc<CompiledModule>, DeserializeError>;
}
