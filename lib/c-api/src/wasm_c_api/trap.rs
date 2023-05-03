use super::store::wasm_store_t;
use super::types::{wasm_byte_vec_t, wasm_message_t};
use super::types::{wasm_frame_t, wasm_frame_vec_t};
use std::ffi::CString;
use wasmer_api::RuntimeError;

// opaque type which is a `RuntimeError`
#[allow(non_camel_case_types)]
pub struct wasm_trap_t {
    pub(crate) inner: RuntimeError,
}

impl From<RuntimeError> for wasm_trap_t {
    fn from(other: RuntimeError) -> Self {
        Self { inner: other }
    }
}

/// Create a new trap message.
///
/// Be careful, the message is typed with `wasm_message_t` which
/// represents a null-terminated string.
///
/// # Example
///
/// See the module's documentation for a complete example.
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_new(
    _store: &mut wasm_store_t,
    message: &wasm_message_t,
) -> Option<Box<wasm_trap_t>> {
    let message_bytes = message.as_slice();

    // The trap message is typed with `wasm_message_t` which is a
    // typeref to `wasm_name_t` with the exception that it's a
    // null-terminated string. `RuntimeError` must contain a valid
    // Rust `String` that doesn't contain a null byte. We must ensure
    // this behavior.
    let runtime_error = match CString::new(message_bytes) {
        // The string is well-formed and doesn't contain a nul byte.
        Ok(cstring) => RuntimeError::new(cstring.into_string().ok()?),

        // The string is well-formed but is nul-terminated. Let's
        // create a `String` which is null-terminated too.
        Err(nul_error) if nul_error.nul_position() + 1 == message_bytes.len() => {
            let mut vec = nul_error.into_vec();
            vec.pop();

            RuntimeError::new(String::from_utf8(vec).ok()?)
        }

        // The string not well-formed.
        Err(_) => return None,
    };

    let trap = runtime_error.into();

    Some(Box::new(trap))
}

/// Deletes a trap.
///
/// # Example
///
/// See the module's documentation for a complete example.
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_delete(_trap: Option<Box<wasm_trap_t>>) {}

/// Gets the message attached to the trap.
///
/// # Example
///
/// ```rust
/// # use wasmer_inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Create an engine and a store.
///     wasm_engine_t* engine = wasm_engine_new();
///     wasm_store_t* store = wasm_store_new(engine);
///
///     // Create the trap message.
///     wasm_message_t message;
///     wasm_name_new_from_string_nt(&message, "foobar");
///
///     // Create the trap with its message.
///     // The backtrace will be generated automatically.
///     wasm_trap_t* trap = wasm_trap_new(store, &message);
///     assert(trap);
///
///     // Get the trap's message back.
///     wasm_message_t retrieved_message;
///     wasm_trap_message(trap, &retrieved_message);
///     assert(retrieved_message.size == message.size);
///
///     // Free everything.
///     wasm_name_delete(&message);
///     wasm_name_delete(&retrieved_message);
///     wasm_trap_delete(trap);
///     wasm_store_delete(store);
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_message(
    trap: &wasm_trap_t,
    // own
    out: &mut wasm_byte_vec_t,
) {
    let message = trap.inner.message();
    let mut byte_vec = message.into_bytes();
    byte_vec.push(0);

    out.set_buffer(byte_vec);
}

/// Gets the origin frame attached to the trap.
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_origin(trap: &wasm_trap_t) -> Option<Box<wasm_frame_t>> {
    trap.inner.trace().first().map(Into::into).map(Box::new)
}

/// Gets the trace (as a list of frames) attached to the trap.
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_trace(
    trap: &wasm_trap_t,
    // own
    out: &mut wasm_frame_vec_t,
) {
    let frames = trap.inner.trace();
    out.set_buffer(
        frames
            .iter()
            .map(|frame| Some(Box::new(frame.into())))
            .collect(),
    );
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_trap_message_null_terminated() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_message_t original_message;
                wasm_name_new_from_string_nt(&original_message, "foobar");
                assert(original_message.size == 7); // 6 for `foobar` + 1 for nul byte.

                wasm_trap_t* trap = wasm_trap_new(store, &original_message);
                assert(trap);

                wasm_message_t retrieved_message;
                wasm_trap_message(trap, &retrieved_message);
                assert(retrieved_message.size == 7);

                wasm_name_delete(&original_message);
                wasm_name_delete(&retrieved_message);
                wasm_trap_delete(trap);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[cfg_attr(coverage, ignore)]
    #[test]
    fn test_trap_message_not_null_terminated() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_message_t original_message;
                wasm_name_new_from_string(&original_message, "foobar");
                assert(original_message.size == 6); // 6 for `foobar` + 0 for nul byte.

                wasm_trap_t* trap = wasm_trap_new(store, &original_message);
                assert(trap);

                wasm_message_t retrieved_message;
                wasm_trap_message(trap, &retrieved_message);
                assert(retrieved_message.size == 7);

                wasm_name_delete(&original_message);
                wasm_name_delete(&retrieved_message);
                wasm_trap_delete(trap);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
