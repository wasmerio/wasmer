//! Read runtime errors.

use libc::{c_char, c_int};
use std::cell::RefCell;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ptr;
use std::slice;

thread_local! {
    static LAST_ERROR: RefCell<Option<Box<dyn Error>>> = RefCell::new(None);
}

pub fn update_last_error<E: Error + 'static>(err: E) {
    LAST_ERROR.with(|prev| {
        *prev.borrow_mut() = Some(Box::new(err));
    });
}

/// Retrieve the most recent error, clearing it in the process.
pub(crate) fn take_last_error() -> Option<Box<dyn Error>> {
    LAST_ERROR.with(|prev| prev.borrow_mut().take())
}

/// Gets the length in bytes of the last error.
/// This can be used to dynamically allocate a buffer with the correct number of
/// bytes needed to store a message.
///
/// # Example
///
/// ```c
/// int error_len = wasmer_last_error_length();
/// char *error_str = malloc(error_len);
/// ```
#[no_mangle]
pub extern "C" fn wasmer_last_error_length() -> c_int {
    LAST_ERROR.with(|prev| match *prev.borrow() {
        Some(ref err) => err.to_string().len() as c_int + 1,
        None => 0,
    })
}

/// Stores the last error message into the provided buffer up to the given `length`.
/// The `length` parameter must be large enough to store the last error message.
///
/// Returns the length of the string in bytes.
/// Returns `-1` if an error occurs.
///
/// # Example
///
/// ```c
/// int error_len = wasmer_last_error_length();
/// char *error_str = malloc(error_len);
/// wasmer_last_error_message(error_str, error_len);
/// printf("Error str: `%s`\n", error_str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_last_error_message(buffer: *mut c_char, length: c_int) -> c_int {
    if buffer.is_null() {
        // buffer pointer is null
        return -1;
    }

    let error_message = match take_last_error() {
        Some(err) => err.to_string(),
        None => return 0,
    };

    let length = length as usize;

    if error_message.len() >= length {
        // buffer is too small to hold the error message
        return -1;
    }

    let buffer = slice::from_raw_parts_mut(buffer as *mut u8, length);

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

#[derive(Debug)]
pub struct CApiError {
    pub msg: String,
}

impl Display for CApiError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", &self.msg)
    }
}

impl Error for CApiError {}
