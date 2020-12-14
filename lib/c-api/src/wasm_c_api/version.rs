use std::os::raw::c_char;

static VERSION: &'static str = env!("CARGO_PKG_VERSION");

/// Get the version of Wasmer.
///
/// The `.h` files already define variables like `WASMER_VERSION*`,
/// but if this file is unreachable, one can use this function to
/// retrieve the full semver version of the Wasmer C API.
///
/// The returned string is statically allocated. It must _not_ be
/// freed!
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
///     // Get and print the version.
///     const char* version = wasmer_version();
///     printf("%s", version);
///
///     // No need to free the string. It's statically allocated on
///     // the Rust side.
///
///     return 0;
/// }
/// #    })
/// #    .success()
/// #    .stdout(env!("CARGO_PKG_VERSION"));
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_version() -> *const c_char {
    VERSION.as_ptr() as *const _
}
