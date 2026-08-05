/// Check if the provided bytes are wasm-like
pub fn is_wasm(bytes: impl AsRef<[u8]>) -> bool {
    bytes.as_ref().starts_with(b"\0asm")
}

/// Remove a leading `#!...\n` shebang line from a self-executing Wasm module.
///
/// Returns an empty slice when the input starts with `#!` but has no terminating
/// newline.
pub fn strip_wasm_shebang(bytes: &[u8]) -> &[u8] {
    if let [b'#', b'!', ..] = bytes {
        match bytes.iter().position(|&byte| byte == b'\n') {
            Some(newline) => &bytes[newline + 1..],
            None => &[],
        }
    } else {
        bytes
    }
}
