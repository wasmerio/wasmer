//! Utility functions for the WebAssembly module

/// Detect if a provided binary is a Wasm file
pub fn is_wasm_binary(binary: &[u8]) -> bool {
    binary.starts_with(&[b'\0', b'a', b's', b'm'])
}
