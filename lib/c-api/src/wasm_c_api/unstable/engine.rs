//! Unstable non-standard Wasmer-specific types for the
//! `wasm_engine_t` and siblings.

use super::{
    super::engine::{wasm_config_t, wasmer_engine_t},
    features::wasmer_features_t,
};

/// Unstable non-standard Wasmer-specific API to update the
/// configuration to specify particular features for the engine.
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
///     // Set the target.
///     {
///         wasmer_features_t* features = wasmer_features_new();
///         wasmer_features_simd(features, true);
///
///         wasm_config_set_features(config, features);
///     }
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
pub extern "C" fn wasm_config_set_features(
    config: &mut wasm_config_t,
    features: Box<wasmer_features_t>,
) {
    config.features = Some(features);
}

/// Check whether there is no compiler available in this compiled
/// library.
#[no_mangle]
pub extern "C" fn wasmer_is_headless() -> bool {
    !cfg!(feature = "compiler")
}

/// Check whether the given engine is available, i.e. part of this
/// compiled library.
#[no_mangle]
pub extern "C" fn wasmer_is_engine_available(engine: wasmer_engine_t) -> bool {
    matches!(engine, wasmer_engine_t::UNIVERSAL if cfg!(feature = "compiler"))
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    use std::env::{remove_var, set_var};
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[test]
    fn test_wasmer_is_headless() {
        set_var(
            "COMPILER",
            if cfg!(feature = "compiler") { "0" } else { "1" },
        );

        (assert_c! {
            #include "tests/wasmer.h"
            #include <stdlib.h>

            int main() {
                assert(wasmer_is_headless() == (getenv("COMPILER")[0] == '1'));

                return 0;
            }
        })
        .success();

        remove_var("COMPILER");
    }

    #[test]
    fn test_wasmer_is_compiler_available() {
        set_var(
            "CRANELIFT",
            if cfg!(feature = "cranelift") {
                "1"
            } else {
                "0"
            },
        );
        set_var("LLVM", if cfg!(feature = "llvm") { "1" } else { "0" });
        set_var(
            "SINGLEPASS",
            if cfg!(feature = "singlepass") {
                "1"
            } else {
                "0"
            },
        );

        (assert_c! {
            #include "tests/wasmer.h"
            #include <stdlib.h>

            int main() {
                assert(wasmer_is_compiler_available(CRANELIFT) == (getenv("CRANELIFT")[0] == '1'));
                assert(wasmer_is_compiler_available(LLVM) == (getenv("LLVM")[0] == '1'));
                assert(wasmer_is_compiler_available(SINGLEPASS) == (getenv("SINGLEPASS")[0] == '1'));

                return 0;
            }
        })
        .success();

        remove_var("CRANELIFT");
        remove_var("LLVM");
        remove_var("SINGLEPASS");
    }

    #[test]
    fn test_wasmer_is_engine_available() {
        set_var(
            "UNIVERSAL",
            if cfg!(feature = "compiler") { "1" } else { "0" },
        );

        (assert_c! {
            #include "tests/wasmer.h"
            #include <stdlib.h>

            int main() {
                assert(wasmer_is_engine_available(UNIVERSAL) == (getenv("UNIVERSAL")[0] == '1'));

                return 0;
            }
        })
        .success();

        remove_var("UNIVERSAL");
    }
}
