use std::os::raw::c_char;
use std::sync::LazyLock;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const VERSION_PRE: &str = concat!(env!("CARGO_PKG_VERSION_PRE"), "\0");

static VERSION_MAJOR: LazyLock<u8> = LazyLock::new(|| {
    env!("CARGO_PKG_VERSION_MAJOR")
        .parse()
        .expect("Failed to parse value for `VERSION_MAJOR` from `CARGO_PKG_VERSION_MAJOR`")
});
static VERSION_MINOR: LazyLock<u8> = LazyLock::new(|| {
    env!("CARGO_PKG_VERSION_MINOR")
        .parse()
        .expect("Failed to parse value for `VERSION_MINOR` from `CARGO_PKG_VERSION_MINOR`")
});
static VERSION_PATCH: LazyLock<u8> = LazyLock::new(|| {
    env!("CARGO_PKG_VERSION_PATCH")
        .parse()
        .expect("Failed to parse value for `VERSION_PATCH` from `CARGO_PKG_VERSION_PATCH`")
});

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
#[unsafe(no_mangle)]
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
/// # use inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// #
/// int main() {
///     // Get and check the version components.
///     uint8_t version_major = wasmer_version_major();
///     uint8_t version_minor = wasmer_version_minor();
///     uint8_t version_patch = wasmer_version_patch();
///
///     assert(version_major == WASMER_VERSION_MAJOR);
///     assert(version_minor == WASMER_VERSION_MINOR);
///     assert(version_patch == WASMER_VERSION_PATCH);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_version_major() -> u8 {
    *VERSION_MAJOR
}

/// Get the minor version of the Wasmer C API.
///
/// See [`wasmer_version_major`] to learn more and get an example.  
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_version_minor() -> u8 {
    *VERSION_MINOR
}

/// Get the patch version of the Wasmer C API.
///
/// See [`wasmer_version_major`] to learn more and get an example.  
#[unsafe(no_mangle)]
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
/// # use inline_c::assert_c;
/// # fn main() {
/// #    (assert_c! {
/// # #include "tests/wasmer.h"
/// # #include <string.h>
/// #
/// int main() {
///     // Get and check the pre version.
///     const char* version_pre = wasmer_version_pre();
///     assert(strcmp(version_pre, WASMER_VERSION_PRE) == 0);
///
///     // No need to free the string. It's statically allocated on
///     // the Rust side.
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasmer_version_pre() -> *const c_char {
    VERSION_PRE.as_ptr() as *const _
}
