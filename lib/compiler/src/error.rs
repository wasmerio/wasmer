use crate::lib::std::string::String;
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
    /// A Wasm translation error occured.
    #[error("WebAssembly translation error: {0}")]
    Wasm(#[from] WasmError),

    /// A compilation error occured.
    #[error("Compilation error: {0}")]
    Codegen(String),

    /// The module did not pass validation.
    #[error("Validation error: {0}")]
    Validate(String),

    /// The compiler doesn't support a Wasm feature
    #[error("Feature {0} is not yet supported")]
    UnsupportedFeature(String),

    /// Insufficient resources available for execution.
    #[error("Insufficient resources: {0}")]
    Resource(String),
}

/// A WebAssembly translation error.
///
/// When a WebAssembly function can't be translated, one of these error codes will be returned
/// to describe the failure.
#[derive(Error, Debug)]
pub enum WasmError {
    /// The input WebAssembly code is invalid.
    ///
    /// This error code is used by a WebAssembly translator when it encounters invalid WebAssembly
    /// code. This should never happen for validated WebAssembly code.
    #[error("Invalid input WebAssembly code at offset {offset}: {message}")]
    InvalidWebAssembly {
        /// A string describing the validation error.
        message: String,
        /// The bytecode offset where the error occurred.
        offset: usize,
    },

    /// A feature used by the WebAssembly code is not supported by the embedding environment.
    ///
    /// Embedding environments may have their own limitations and feature restrictions.
    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    /// An implementation limit was exceeded.
    #[error("Implementation limit exceeded")]
    ImplLimitExceeded,

    /// A generic error.
    #[error("{0}")]
    Generic(String),
}

/// A convenient alias for a `Result` that uses `WasmError` as the error type.
pub type WasmResult<T> = Result<T, WasmError>;
