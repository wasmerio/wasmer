use crate::std::string::String;
use crate::translator::WasmError;
use thiserror::Error;

// Compilation Errors

/// The WebAssembly.CompileError object indicates an error during
/// WebAssembly decoding or validation.
///
/// This is based on the [Wasm Compile Error][compile-error] API.
///
/// [compiler-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/CompileError
#[derive(Error, Debug)]
pub enum CompileError {
    /// A wasm translation error occured.
    #[error("WebAssembly translation error: {0}")]
    Wasm(#[from] WasmError),

    /// A compilation error occured.
    #[error("Compilation error: {0}")]
    Codegen(String),

    /// The module did not pass validation.
    #[error("Validation error: {0}")]
    Validate(String),

    /// Insufficient resources available for execution.
    #[error("Insufficient resources: {0}")]
    Resource(String),
}
