use crate::WasmError;
use wasmparser::BinaryReaderError;

/// Return an `Err(WasmError::Unsupported(msg))` where `msg` the string built by calling `format!`
/// on the arguments to this macro.
#[macro_export]
macro_rules! wasm_unsupported {
    ($($arg:tt)*) => { $crate::WasmError::Unsupported(format!($($arg)*)) }
}

impl From<BinaryReaderError> for WasmError {
    fn from(original: BinaryReaderError) -> Self {
        Self::InvalidWebAssembly {
            message: original.message().into(),
            offset: original.offset(),
        }
    }
}
