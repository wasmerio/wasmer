pub use super::unstable::engine::wasm_config_set_features;
use super::unstable::features::wasmer_features_t;

use wasmer_api::Engine;

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
pub enum wasmer_engine_t {
    /// The sys (cranelift, llvm, or singlepass) backend.
    UNIVERSAL = 0,

    /// The V8 backend.
    V8,

    /// The WASMI backend.
    WASMI,

    /// The WAMR backend.
    WAMR,

    /// The JSC backend.
    JSC,
}

impl Default for wasmer_engine_t {
    fn default() -> Self {
        // Let the `wasmer_api` crate decide which is the default engine, given the enabled
        // features.
        match wasmer_api::BackendKind::default() {
            #[cfg(feature = "sys")]
            wasmer_api::BackendKind::Sys => wasmer_engine_t::UNIVERSAL,
            #[cfg(feature = "wamr")]
            wasmer_api::BackendKind::Wamr => wasmer_engine_t::WAMR,
            #[cfg(feature = "wasmi")]
            wasmer_api::BackendKind::Wasmi => wasmer_engine_t::WASMI,
            #[cfg(feature = "v8")]
            wasmer_api::BackendKind::V8 => wasmer_engine_t::V8,
            #[cfg(feature = "jsc")]
            wasmer_api::BackendKind::Jsc => wasmer_engine_t::JSC,
            // Should be unreachable, however.
            b => panic!("Unsupported backend: {b:?}"),
        }
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

#[no_mangle]
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
#[no_mangle]
pub unsafe extern "C" fn wasm_engine_delete(_engine: Option<Box<wasm_engine_t>>) {}

/// Creates an engine with a particular configuration.
///
/// # Example
///
/// See [`wasm_config_new`].
///
/// cbindgen:ignore
#[no_mangle]
pub extern "C" fn wasm_engine_new_with_config(
    config: Option<Box<wasm_config_t>>,
) -> Option<Box<wasm_engine_t>> {
    #[allow(unused)]
    let config = *(config?);

    match config.engine {
        #[cfg(feature = "sys")]
        wasmer_engine_t::UNIVERSAL => config::sys::wasm_sys_engine_new_with_config(config),
        #[cfg(feature = "v8")]
        wasmer_engine_t::V8 => config::v8::wasm_v8_engine_new_with_config(config),
        #[cfg(feature = "wasmi")]
        wasmer_engine_t::WASMI => config::wasmi::wasm_wasmi_engine_new_with_config(config),
        #[cfg(feature = "wamr")]
        wasmer_engine_t::WAMR => config::wamr::wasm_wamr_engine_new_with_config(config),
        #[cfg(feature = "jsc")]
        wasmer_engine_t::JSC => config::jsc::wasm_jsc_engine_new_with_config(config),
        _ => unreachable!(),
    }
}

/// A configuration holds the compiler and the engine used by the store.
///
/// cbindgen:ignore
#[derive(Debug, Default)]
#[repr(C)]
pub struct wasm_config_t {
    pub(super) engine: wasmer_engine_t,
    pub(super) engine_config: wasmer_engine_config_t,
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
pub extern "C" fn wasm_config_set_engine(config: &mut wasm_config_t, engine: wasmer_engine_t) {
    config.engine = engine;
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[cfg_attr(coverage, ignore)]
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
