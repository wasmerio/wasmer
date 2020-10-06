use super::types::wasm_byte_vec_t;

/// Parses in-memory bytes as either the WAT format, or a binary Wasm
/// module. This is wasmer-specific.
///
/// In case of failure, `wat2wasm` returns `NULL`.
#[cfg(feature = "wat")]
#[no_mangle]
pub unsafe extern "C" fn wat2wasm(wat: &wasm_byte_vec_t) -> Option<Box<wasm_byte_vec_t>> {
    let wat: &[u8] = wat.into_slice()?;

    let result = match wasmer::wat2wasm(wat) {
        Ok(result) => result,
        Err(error) => {
            crate::error::update_last_error(error);

            return None;
        }
    };

    let mut result: Vec<u8> = result.into_owned();
    result.shrink_to_fit();

    Some(Box::new(wasm_byte_vec_t {
        size: result.len(),
        data: result.as_mut_ptr(),
    }))
}
