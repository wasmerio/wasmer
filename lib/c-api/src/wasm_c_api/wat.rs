use super::types::wasm_byte_vec_t;

/// Parses in-memory bytes as either the WAT format, or a binary Wasm
/// module. This is wasmer-specific.
///
/// In case of failure, `wat2wasm` returns `NULL`.
#[cfg(feature = "wat")]
#[no_mangle]
pub unsafe extern "C" fn wat2wasm(wat: &wasm_byte_vec_t) -> Option<Box<wasm_byte_vec_t>> {
    let wat: &[u8] = wat.into_slice()?;
    let result: wasm_byte_vec_t = c_try!(wasmer::wat2wasm(wat)).into_owned().into();

    Some(Box::new(result))
}
