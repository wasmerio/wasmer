//! Utilities to read errors.
//!
//! Only one error can be registered at a time. Error are registered
//! by Rust only, and are usually read by C or C++.
//!
//! Reading an error from C or C++ happens in 2 steps: Getting the
//! error's length with [`wasmer_last_error_length`], and then reading
//! the actual error with [`wasmer_last_error_message`].
//!
//! # Example
//!
//! ```rust
//! # use inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer.h"
//! #
//! int main() {
//!     // Create an invalid WebAssembly module from a WAT definition,
//!     // it will generate an error!
//!     wasm_byte_vec_t wat;
//!     wasmer_byte_vec_new_from_string(&wat, "(foobar)");
//!     wasm_byte_vec_t wasm;
//!     wat2wasm(&wat, &wasm);
//!
//!     int error_length = wasmer_last_error_length();
//!
//!     // There is an error!
//!     assert(error_length > 0);
//!
//!     char *error_message = malloc(error_length);
//!     wasmer_last_error_message(error_message, error_length);
//!     printf("Error message: %s\n", error_message);
//!
//!     // Side note: The error has now been cleared on the Rust side!
//!     assert(wasmer_last_error_length() == 0);
//!
//!     // Free everything.
//!     free(error_message);
//!     wasm_byte_vec_delete(&wasm);
//!     wasm_byte_vec_delete(&wat);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```

use libc::{c_char, c_int};
use std::cell::RefCell;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ptr::{self, NonNull};
use std::slice;

thread_local! {
    static LAST_ERROR: RefCell<Option<Box<dyn Error>>> = RefCell::new(None);
}

/// Rust function to register a new error.
///
/// # Example
///
/// ```rust,no_run
/// # use wasmer_c_api::error::{update_last_error, CApiError};
///
/// update_last_error(CApiError {
///     msg: "Hello, World!".to_string(),
/// });
/// ```
pub fn update_last_error<E: Error + 'static>(err: E) {
    LAST_ERROR.with(|prev| {
        *prev.borrow_mut() = Some(Box::new(err));
    });
}

/// Retrieve the most recent error, clearing it in the process.
pub(crate) fn take_last_error() -> Option<Box<dyn Error>> {
    LAST_ERROR.with(|prev| prev.borrow_mut().take())
}

/// Gets the length in bytes of the last error if any, zero otherwise.
///
/// This can be used to dynamically allocate a buffer with the correct number of
/// bytes needed to store a message.
///
/// # Example
///
/// See this module's documentation to get a complete example.
#[no_mangle]
pub extern "C" fn wasmer_last_error_length() -> c_int {
    LAST_ERROR.with(|prev| match *prev.borrow() {
        Some(ref err) => err.to_string().len() as c_int + 1,
        None => 0,
    })
}

/// Gets the last error message if any into the provided buffer
/// `buffer` up to the given `length`.
///
/// The `length` parameter must be large enough to store the last
/// error message. Ideally, the value should come from
/// [`wasmer_last_error_length`].
///
/// The function returns the length of the string in bytes, `-1` if an
/// error occurs. Potential errors are:
///
///  * The `buffer` is a null pointer,
///  * The `buffer` is too small to hold the error message.
///
/// Note: The error message always has a trailing NUL character.
///
/// Important note: If the provided `buffer` is non-null, once this
/// function has been called, regardless whether it fails or succeeds,
/// the error is cleared.
///
/// # Example
///
/// See this module's documentation to get a complete example.
#[no_mangle]
pub unsafe extern "C" fn wasmer_last_error_message(
    buffer: Option<NonNull<c_char>>,
    length: c_int,
) -> c_int {
    let buffer = if let Some(buffer_inner) = buffer {
        buffer_inner
    } else {
        // buffer pointer is null
        return -1;
    };

    let error_message = match take_last_error() {
        Some(err) => err.to_string(),
        None => return 0,
    };

    let length = length as usize;

    if error_message.len() >= length {
        // buffer is too small to hold the error message
        return -1;
    }

    let buffer = slice::from_raw_parts_mut(buffer.cast::<u8>().as_ptr(), length);

    ptr::copy_nonoverlapping(
        error_message.as_ptr(),
        buffer.as_mut_ptr(),
        error_message.len(),
    );

    // Add a trailing null so people using the string as a `char *` don't
    // accidentally read into garbage.
    buffer[error_message.len()] = 0;

    error_message.len() as c_int + 1
}

/// Rust type to represent a C API error.
#[derive(Debug)]
pub struct CApiError {
    /// The error message.
    pub msg: String,
}

impl Display for CApiError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", &self.msg)
    }
}

impl Error for CApiError {}
