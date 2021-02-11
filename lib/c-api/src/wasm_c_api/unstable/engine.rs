//! Unstable non-standard Wasmer-specific types for the
//! `wasm_engine_t` and siblings.

use super::super::engine::{wasm_config_t, wasmer_compiler_t, wasmer_engine_t};
use super::target_lexicon::wasm_target_t;

/// Unstable non-standard Wasmer-specific API to update the
/// configuration to specify a particular target for the engine.
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
///     // Create the configuration.
///     wasm_config_t* config = wasm_config_new();
///
///     // Set the target.
///     {
///         wasm_triple_t* triple = wasm_triple_new_from_host();
///         wasm_cpu_features_t* cpu_features = wasm_cpu_features_new();
///         wasm_target_t* target = wasm_target_new(triple, cpu_features);
///
///         wasm_config_set_target(config, target);
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
pub extern "C" fn wasm_config_set_target(config: &mut wasm_config_t, target: Box<wasm_target_t>) {
    config.target = Some(target);
}

#[no_mangle]
pub extern "C" fn wasmer_is_compiler_available(compiler: wasmer_compiler_t) -> bool {
    match compiler {
        wasmer_compiler_t::CRANELIFT if cfg!(feature = "cranelift") => true,
        wasmer_compiler_t::LLVM if cfg!(feature = "llvm") => true,
        wasmer_compiler_t::SINGLEPASS if cfg!(feature = "singlepass") => true,
        _ => false,
    }
}

#[no_mangle]
pub extern "C" fn wasmer_is_engine_available(engine: wasmer_engine_t) -> bool {
    match engine {
        wasmer_engine_t::JIT if cfg!(feature = "jit") => true,
        wasmer_engine_t::NATIVE if cfg!(feature = "native") => true,
        wasmer_engine_t::OBJECT_FILE if cfg!(feature = "object-file") => true,
        _ => false,
    }
}
