use crate::WasmError;
use thiserror::Error;
use wasmparser::BinaryReaderError;

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
