pub use super::unstable::engine::wasm_config_set_features;
use super::unstable::features::wasmer_features_t;

use wasmer_api::{BackendKind, Engine};

mod config;
#[allow(unused_imports)]
pub use config::*;

/// Kind of engines that can be used by the store.
///
/// This is a Wasmer-specific type with Wasmer-specific functions for
/// manipulating it.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[allow(non_camel_case_types)]
pub enum wasmer_backend_t {
    /// The cranelift backend.
    CRANELIFT = 0,

    /// The LLVM backend.
    LLVM,

    /// The singlepass backend.
    SINGLEPASS,

    /// The headless backend.
    HEADLESS,

    /// The V8 backend.
    V8,

    /// The WASMI backend.
    WASMI,

    /// The WAMR backend.
    WAMR,

    /// The JSC backend.
    JSC,
}

impl From<BackendKind> for wasmer_backend_t {
    fn from(value: BackendKind) -> Self {
        match value {
            #[cfg(feature = "cranelift")]
            BackendKind::Cranelift => Self::CRANELIFT,
            #[cfg(feature = "llvm")]
            BackendKind::LLVM => Self::LLVM,
            #[cfg(feature = "singlepass")]
            BackendKind::Singlepass => Self::SINGLEPASS,
            #[cfg(feature = "sys")]
            BackendKind::Headless => Self::HEADLESS,
            #[cfg(feature = "wamr")]
            BackendKind::Wamr => Self::WAMR,
            #[cfg(feature = "wasmi")]
            BackendKind::Wasmi => Self::WASMI,
            #[cfg(feature = "v8")]
            BackendKind::V8 => Self::V8,
            #[cfg(feature = "jsc")]
            BackendKind::Jsc => Self::JSC,
            v => panic!("Unsupported backend kind {v:?}"),
        }
    }
}

impl Default for wasmer_backend_t {
    fn default() -> Self {
        // Let the `wasmer_api` crate decide which is the default engine, given the enabled
        // features.
        wasmer_api::BackendKind::default().into()
    }
}

/// An engine is used by the store to drive the compilation and the
/// execution of a WebAssembly module.
///
/// cbindgen:ignore
#[repr(C)]
// We can let the API decide which engine is default with the given set of
// features.
//
// See the impl of `Default` for `BackendEngine` in the `wasmer` (API) crate.
#[derive(Default)]
pub struct wasm_engine_t {
    pub(crate) inner: Engine,
}

#[unsafe(no_mangle)]
#[allow(unreachable_code)]
pub extern "C" fn wasm_engine_new() -> Box<wasm_engine_t> {
    Box::new(wasm_engine_t::default())
}

/// Deletes an engine.
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
///     // Create a default engine.
///     wasm_engine_t* engine = wasm_engine_new();
///     int error_length = wasmer_last_error_length();
///     if (error_length > 0) {
///         char *error_message = malloc(error_length);
///         wasmer_last_error_message(error_message, error_length);
///
///         printf("Attempted to set an immutable global: `%s`\n", error_message);
///         free(error_message);
///     }
///
///     // Check we have an engine!
///     assert(engine);
///
///     // Free everything.
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_engine_delete(_engine: Option<Box<wasm_engine_t>>) {}

/// Creates an engine with a particular configuration.
///
/// # Example
///
/// See [`wasm_config_new`].
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub extern "C" fn wasm_engine_new_with_config(
    config: Option<Box<wasm_config_t>>,
) -> Option<Box<wasm_engine_t>> {
    #[allow(unused)]
    let config = *(config?);

    match config.backend {
        #[cfg(feature = "llvm")]
        wasmer_backend_t::LLVM => config::sys::wasm_sys_engine_new_with_config(config),
        #[cfg(feature = "cranelift")]
        wasmer_backend_t::CRANELIFT => config::sys::wasm_sys_engine_new_with_config(config),
        #[cfg(feature = "singlepass")]
        wasmer_backend_t::SINGLEPASS => config::sys::wasm_sys_engine_new_with_config(config),
        #[cfg(feature = "sys")]
        wasmer_backend_t::HEADLESS => config::sys::wasm_sys_engine_new_with_config(config),
        #[cfg(feature = "v8")]
        wasmer_backend_t::V8 => config::v8::wasm_v8_engine_new_with_config(config),
        #[cfg(feature = "wasmi")]
        wasmer_backend_t::WASMI => config::wasmi::wasm_wasmi_engine_new_with_config(config),
        #[cfg(feature = "wamr")]
        wasmer_backend_t::WAMR => config::wamr::wasm_wamr_engine_new_with_config(config),
        #[cfg(feature = "jsc")]
        wasmer_backend_t::JSC => config::jsc::wasm_jsc_engine_new_with_config(config),
        #[allow(unreachable_patterns)]
        _ => unreachable!(),
    }
}

/// A configuration holds the compiler and the engine used by the store.
///
/// cbindgen:ignore
#[derive(Debug, Default)]
#[repr(C)]
pub struct wasm_config_t {
    pub(super) backend: wasmer_backend_t,
    pub(super) backend_config: wasmer_backend_config_t,
    pub(super) features: Option<Box<wasmer_features_t>>,
}

/// Create a new default Wasmer configuration.
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
///     // Create the configuration.
///     wasm_config_t* config = wasm_config_new();
///
///     // Create the engine.
///     wasm_engine_t* engine = wasm_engine_new_with_config(config);
///
///     // Check we have an engine!
///     assert(engine);
///
///     // Free everything.
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub extern "C" fn wasm_config_new() -> Box<wasm_config_t> {
    Box::<wasm_config_t>::default()
}

/// Delete a Wasmer config object.
///
/// This function does not need to be called if `wasm_engine_new_with_config` or
/// another function that takes ownership of the `wasm_config_t` is called.
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
///     // Create the configuration.
///     wasm_config_t* config = wasm_config_new();
///
///     // Delete the configuration
///     wasm_config_delete(config);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
/// cbindgen:ignore
#[unsafe(no_mangle)]
pub extern "C" fn wasm_config_delete(_config: Option<Box<wasm_config_t>>) {}

/// Updates the configuration to specify a particular engine to use.
///
/// This is a Wasmer-specific function.
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
///     // Create the configuration.
///     wasm_config_t* config = wasm_config_new();
///
///     // Create the engine.
///     wasm_engine_t* engine = wasm_engine_new_with_config(config);
///
///     // Check we have an engine!
///     assert(engine);
///
///     // Free everything.
///     wasm_engine_delete(engine);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn wasm_config_set_backend(config: &mut wasm_config_t, engine: wasmer_backend_t) {
    config.backend = engine;
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[allow(
        unexpected_cfgs,
        reason = "tools like cargo-llvm-coverage pass --cfg coverage"
    )]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[test]
    fn test_engine_new() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                assert(engine);

                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
