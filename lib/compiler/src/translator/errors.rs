use crate::std::string::String;
use thiserror::Error;
use wasmparser::BinaryReaderError;

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
/// Return an `Err(WasmError::Unsupported(msg))` where `msg` the string built by calling `format!`
/// on the arguments to this macro.
#[macro_export]
macro_rules! wasm_unsupported {
    ($($arg:tt)*) => { $crate::WasmError::Unsupported(format!($($arg)*)) }
}

/// Converts a Wasm binary reading error to a runtime Wasm error
pub fn to_wasm_error(e: BinaryReaderError) -> WasmError {
    WasmError::InvalidWebAssembly {
        message: e.message().into(),
        offset: e.offset(),
    }
}

/// A convenient alias for a `Result` that uses `WasmError` as the error type.
pub type WasmResult<T> = Result<T, WasmError>;
