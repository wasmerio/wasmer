//! Wasmer C API.
//!
//! [Wasmer](https://github.com/wasmerio/wasmer) is the leading
//! WebAssembly runtime. This crate exposes the C and C++ bindings.
//!
//! This crate provides 2 API:
//!
//! * `deprecated`, which is the old one, and is represented by the
//!   `wasmer.h` and the `wasmer.hh` C header files,
//! * `wasm_c_api`, which is the new, standard C API, and is
//!   represented by the `wasmer_wasm.h` C header file.
//!
//! The `wasm_c_api` follows the [official WebAssembly C and C++
//! API](https://github.com/WebAssembly/wasm-c-api). It provides
//! however some extensions, like the `wasi_*` types and functions,
//! which aren't yet defined by the standard. The `wasmer_wasm.h`
//! header file already depends on the `wasm.h` file. A copy lands in
//! this repository for the sake of simplicity.

#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
// temporary while in transition
#![allow(unused_variables)]
#![deny(
    dead_code,
    unused_imports,
    // temporarily disabled
    //unused_variables,
    unused_unsafe,
    unreachable_patterns
)]

use std::ffi::CString;
use std::mem;
use std::os::raw::c_char;

#[cfg(feature = "deprecated")]
pub mod deprecated;
pub mod error;
mod ordered_resolver;
pub mod wasm_c_api;

/// Gets the version of the Wasmer C API.
///
/// The returned string is guaranteed to be a valid C string. The
/// caller owns the string, so it's up to the caller to free it.
///
/// # Example
///
/// ```rust
/// # use inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer_wasm.h"
/// #
/// int main() {
///     // Read and print the version.
///     char* version = wasmer_version();
///     printf("%s", version);
///
///     // Free it, as we own it.
///     free(version);
///
///     return 0;
/// }
/// #    })
/// #    .success()
/// #    .stdout(std::env::var("CARGO_PKG_VERSION").unwrap());
/// # }
/// ```
#[no_mangle]
pub extern "C" fn wasmer_version() -> *mut c_char {
    let version = CString::new(env!("CARGO_PKG_VERSION"))
        .expect("Failed to create a CString for the Wasmer C version");
    let ptr = version.as_ptr();

    mem::forget(version);

    ptr as *mut _
}
