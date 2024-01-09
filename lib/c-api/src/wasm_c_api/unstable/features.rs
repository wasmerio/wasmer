//! Unstable non-standard Wasmer-specific API that contains a Features
//! API for the engine and the compiler.
//!
//!
//! # Example
//!
//! ```rust
//! # use wasmer_inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer.h"
//! #
//! int main() {
//!     // Declare features.
//!     wasmer_features_t* features = wasmer_features_new();
//!
//!     // Now, let's enable de SIMD feature.
//!     wasmer_features_simd(features, true);
//!
//!     // And also the memory64 feature.     
//!     wasmer_features_memory64(features, true);
//!
//!     wasmer_features_delete(features);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```
//!
//! To go further, see
//! [`wasm_config_set_features`](super::engine::wasm_config_set_features).

use wasmer_types::Features;

/// Controls which experimental features will be enabled.
/// Features usually have a corresponding [WebAssembly proposal].
///
/// [WebAssembly proposal]: https://github.com/WebAssembly/proposals
///
/// # Example
///
/// See the module's documentation.
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasmer_features_t {
    pub(crate) inner: Features,
}

/// Creates a new [`wasmer_features_t`].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_new() -> Box<wasmer_features_t> {
    Box::new(wasmer_features_t {
        inner: Features::new(),
    })
}

/// Delete a [`wasmer_features_t`].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_delete(_features: Option<Box<wasmer_features_t>>) {}

/// Configures whether the WebAssembly threads proposal will be enabled.
///
/// The [WebAssembly threads proposal][threads] is not currently fully
/// standardized and is undergoing development. Support for this feature can
/// be disabled through this method for appropriate WebAssembly modules.
///
/// This feature gates items such as shared memories and atomic
/// instructions.
///
/// This is `true` by default.
///
/// [threads]: https://github.com/webassembly/threads
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_threads(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.threads(enable);

    true
}

/// Configures whether the WebAssembly reference types proposal will be
/// enabled.
///
/// The [WebAssembly reference types proposal][proposal] is not currently
/// fully standardized and is undergoing development. Support for this
/// feature can be enabled through this method for appropriate WebAssembly
/// modules.
///
/// This feature gates items such as the `externref` type and multiple tables
/// being in a module. Note that enabling the reference types feature will
/// also enable the bulk memory feature.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/reference-types
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_reference_types(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.reference_types(enable);

    true
}

/// Configures whether the WebAssembly SIMD proposal will be
/// enabled.
///
/// The [WebAssembly SIMD proposal][proposal] is not currently
/// fully standardized and is undergoing development. Support for this
/// feature can be enabled through this method for appropriate WebAssembly
/// modules.
///
/// This feature gates items such as the `v128` type and all of its
/// operators being in a module.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/simd
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_simd(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.simd(enable);

    true
}

/// Configures whether the WebAssembly bulk memory operations proposal will
/// be enabled.
///
/// The [WebAssembly bulk memory operations proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature gates items such as the `memory.copy` instruction, passive
/// data/table segments, etc, being in a module.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/bulk-memory-operations
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_bulk_memory(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.bulk_memory(enable);

    true
}

/// Configures whether the WebAssembly multi-value proposal will
/// be enabled.
///
/// The [WebAssembly multi-value proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature gates functions and blocks returning multiple values in a
/// module, for example.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/multi-value
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_multi_value(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.multi_value(enable);

    true
}

/// Configures whether the WebAssembly tail-call proposal will
/// be enabled.
///
/// The [WebAssembly tail-call proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature gates tail-call functions in WebAssembly.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/tail-call
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_tail_call(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.tail_call(enable);

    true
}

/// Configures whether the WebAssembly tail-call proposal will
/// be enabled.
///
/// The [WebAssembly tail-call proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature allows WebAssembly modules to define, import and
/// export modules and instances.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/webassembly/module-linking
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_module_linking(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.module_linking(enable);

    true
}

/// Configures whether the WebAssembly multi-memory proposal will
/// be enabled.
///
/// The [WebAssembly multi-memory proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature adds the ability to use multiple memories within a
/// single Wasm module.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/WebAssembly/multi-memory
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_multi_memory(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.multi_memory(enable);

    true
}

/// Configures whether the WebAssembly 64-bit memory proposal will
/// be enabled.
///
/// The [WebAssembly 64-bit memory proposal][proposal] is not
/// currently fully standardized and is undergoing development.
/// Support for this feature can be enabled through this method for
/// appropriate WebAssembly modules.
///
/// This feature gates support for linear memory of sizes larger than
/// 2^32 bits.
///
/// This is `false` by default.
///
/// [proposal]: https://github.com/WebAssembly/memory64
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_features_memory64(
    features: Option<&mut wasmer_features_t>,
    enable: bool,
) -> bool {
    let features = match features {
        Some(features) => features,
        _ => return false,
    };

    features.inner.memory64(enable);

    true
}
