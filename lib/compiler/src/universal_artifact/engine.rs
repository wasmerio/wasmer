//! Universal compilation.

use crate::Compiler;
use wasmer_types::{CompileError, Features};

/// The Builder contents of `Engine`
pub struct EngineBuilder {
    /// The compiler
    compiler: Option<Box<dyn Compiler>>,
    /// The features to compile the Wasm module with
    features: Features,
}

impl EngineBuilder {
    /// Create a new builder with pre-made components
    pub fn new(compiler: Option<Box<dyn Compiler>>, features: Features) -> Self {
        Self { compiler, features }
    }

    /// Gets the compiler associated to this engine.
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        if self.compiler.is_none() {
            return Err(CompileError::Codegen(
                "The Engine is not compiled in.".to_string(),
            ));
        }
        Ok(&**self.compiler.as_ref().unwrap())
    }

    /// Validate the module
    pub fn validate(&self, data: &[u8]) -> Result<(), CompileError> {
        self.compiler()?.validate_module(self.features(), data)
    }

    /// The Wasm features
    pub fn features(&self) -> &Features {
        &self.features
    }
}
