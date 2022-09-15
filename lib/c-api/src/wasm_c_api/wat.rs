use super::types::wasm_byte_vec_t;

/// Parses in-memory bytes as either the WAT format, or a binary Wasm
/// module. This is wasmer-specific.
///
/// In case of failure, `wat2wasm` sets the `out->data = NULL` and `out->size = 0`.
///
/// # Example
///
/// See the module's documentation.
///
/// # Safety
/// This function is unsafe in order to be callable from C.
#[cfg(feature = "wat")]
#[no_mangle]
pub unsafe extern "C" fn wat2wasm(wat: &wasm_byte_vec_t, out: &mut wasm_byte_vec_t) {
    match wasmer_api::wat2wasm(wat.as_slice()) {
        Ok(val) => out.set_buffer(val.into_owned()),
        Err(err) => {
            crate::error::update_last_error(err);
            out.data = std::ptr::null_mut();
            out.size = 0;
        }
    };
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[test]
    fn test_wat2wasm() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module)");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                assert(wasm.data);
                assert(wasm.size == 8);
                assert(
                    wasm.data[0] == 0 &&
                        wasm.data[1] == 'a' &&
                        wasm.data[2] == 's' &&
                        wasm.data[3] == 'm' &&
                        wasm.data[4] == 1 &&
                        wasm.data[5] == 0 &&
                        wasm.data[6] == 0 &&
                        wasm.data[7] == 0
                );

                wasm_byte_vec_delete(&wasm);
                wasm_byte_vec_delete(&wat);

                return 0;
            }
        })
        .success();
    }

    #[test]
    fn test_wat2wasm_failed() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module");
                wasm_byte_vec_t wasm;
                wat2wasm(&wat, &wasm);

                assert(!wasm.data);
                assert(wasmer_last_error_length() > 0);

                wasm_byte_vec_delete(&wat);

                return 0;
            }
        })
        .success();
    }
}
