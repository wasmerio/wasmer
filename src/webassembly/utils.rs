
/// Detect if a provided binary is a WASM file
pub fn is_wasm_binary(binary: &Vec<u8>) -> bool {
    binary.starts_with(&[b'\0', b'a', b's', b'm'])
}
