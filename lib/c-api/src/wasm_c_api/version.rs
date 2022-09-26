use lazy_static::lazy_static;
use std::os::raw::c_char;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const VERSION_PRE: &str = concat!(env!("CARGO_PKG_VERSION_PRE"), "\0");

lazy_static! {
    static ref VERSION_MAJOR: u8 = env!("CARGO_PKG_VERSION_MAJOR")
        .parse()
        .expect("Failed to parse value for `VERSION_MAJOR` from `CARGO_PKG_VERSION_MAJOR`");
    static ref VERSION_MINOR: u8 = env!("CARGO_PKG_VERSION_MINOR")
        .parse()
        .expect("Failed to parse value for `VERSION_MINOR` from `CARGO_PKG_VERSION_MINOR`");
    static ref VERSION_PATCH: u8 = env!("CARGO_PKG_VERSION_PATCH")
        .parse()
        .expect("Failed to parse value for `VERSION_PATCH` from `CARGO_PKG_VERSION_PATCH`");
}

/// Get the version of the Wasmer C API.
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
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasmer_version() -> *const c_char {
    VERSION.as_ptr() as *const _
}

/// Get the major version of the Wasmer C API.
///
/// See [`wasmer_version`] to learn more.
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
///     // Get and print the version components.
///     uint8_t version_major = wasmer_version_major();
///     uint8_t version_minor = wasmer_version_minor();
///     uint8_t version_patch = wasmer_version_patch();
///
///     printf("%d.%d.%d", version_major, version_minor, version_patch);
///
///     return 0;
/// }
/// #    })
/// #    .success()
/// #    .stdout(
/// #         format!(
/// #             "{}.{}.{}",
/// #             env!("CARGO_PKG_VERSION_MAJOR"),
/// #             env!("CARGO_PKG_VERSION_MINOR"),
/// #             env!("CARGO_PKG_VERSION_PATCH")
/// #         )
/// #     );
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_version_major() -> u8 {
    *VERSION_MAJOR
}

/// Get the minor version of the Wasmer C API.
///
/// See [`wasmer_version_major`] to learn more and get an example.  
#[no_mangle]
pub unsafe extern "C" fn wasmer_version_minor() -> u8 {
    *VERSION_MINOR
}

/// Get the patch version of the Wasmer C API.
///
/// See [`wasmer_version_major`] to learn more and get an example.  
#[no_mangle]
pub unsafe extern "C" fn wasmer_version_patch() -> u8 {
    *VERSION_PATCH
}

/// Get the minor version of the Wasmer C API.
///
/// See [`wasmer_version_major`] to learn more.
///
/// The returned string is statically allocated. It must _not_ be
/// freed!
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
///     // Get and print the pre version.
///     const char* version_pre = wasmer_version_pre();
///     printf("%s", version_pre);
///
///     // No need to free the string. It's statically allocated on
///     // the Rust side.
///
///     return 0;
/// }
/// #    })
/// #    .success()
/// #    .stdout(env!("CARGO_PKG_VERSION_PRE"));
/// # }
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_version_pre() -> *const c_char {
    VERSION_PRE.as_ptr() as *const _
}
