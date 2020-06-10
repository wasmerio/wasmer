//! JIT compilation.

use crate::tunables::Tunables;
use crate::{Artifact, DeserializeError};
use std::path::Path;
use std::sync::Arc;
use wasm_common::FunctionType;
use wasmer_compiler::CompileError;
use wasmer_runtime::{VMSharedSignatureIndex, VMTrampoline};

/// A unimplemented Wasmer `Engine`.
///
/// This trait is used by implementors to implement custom engines
/// such as: JIT or Native.
///
/// The product that an `Engine` produces and consumes is the [`Artifact`].
pub trait Engine {
    /// Get the tunables
    fn tunables(&self) -> &dyn Tunables;

    /// Register a signature
    fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex;

    /// Lookup a signature
    fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType>;

    /// Retrieves a trampoline given a signature
    fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline>;

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError>;

    /// Compile a WebAssembly binary
    fn compile(&self, binary: &[u8]) -> Result<Arc<dyn Artifact>, CompileError>;

    /// Deserializes a WebAssembly module
    ///
    /// # Safety
    ///
    /// The serialized content must represent a serialized WebAssembly module.
    unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError>;

    /// Deserializes a WebAssembly module from a path
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<dyn Artifact>, DeserializeError> {
        let bytes = std::fs::read(file_ref)?;
        self.deserialize(&bytes)
    }
}
