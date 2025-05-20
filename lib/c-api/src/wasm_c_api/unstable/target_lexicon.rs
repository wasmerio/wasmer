//! Unstable non-standard Wasmer-specific API that contains everything
//! to create a target with a triple and CPU features.
//!
//! This is useful for cross-compilation.
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
//!     // Declare the target triple.
//!     wasmer_triple_t* triple;
//!
//!     {
//!         wasm_name_t triple_name;
//!         wasm_name_new_from_string(&triple_name, "x86_64-apple-darwin");
//!
//!         triple = wasmer_triple_new(&triple_name);
//!
//!         wasm_name_delete(&triple_name);
//!     }
//!
//!     assert(triple);
//!
//!     // Declare the target CPU features.
//!     wasmer_cpu_features_t* cpu_features = wasmer_cpu_features_new();
//!
//!     {
//!         wasm_name_t cpu_feature_name;
//!         wasm_name_new_from_string(&cpu_feature_name, "sse2");
//!
//!         wasmer_cpu_features_add(cpu_features, &cpu_feature_name);
//!
//!         wasm_name_delete(&cpu_feature_name);
//!     }
//!
//!     assert(cpu_features);
//!
//!     // Create the target!
//!     wasmer_target_t* target = wasmer_target_new(triple, cpu_features);
//!     assert(target);
//!
//!     wasmer_target_delete(target);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```

use super::super::types::wasm_name_t;
use enumset::EnumSet;
use std::slice;
use std::str::{self, FromStr};
use wasmer_types::target::{CpuFeature, Target, Triple};

/// Unstable non-standard Wasmer-specific API to represent a triple +
/// CPU features pair.
///
/// # Example
///
/// See the module's documentation.
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct wasmer_target_t {
    #[allow(unused)]
    pub(crate) inner: Target,
}

/// Creates a new [`wasmer_target_t`].
///
/// It takes ownership of `triple` and `cpu_features`.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_target_new(
    triple: Option<Box<wasmer_triple_t>>,
    cpu_features: Option<Box<wasmer_cpu_features_t>>,
) -> Option<Box<wasmer_target_t>> {
    let triple = triple?;
    let cpu_features = cpu_features?;

    Some(Box::new(wasmer_target_t {
        inner: Target::new(triple.inner.clone(), cpu_features.inner),
    }))
}

/// Delete a [`wasmer_target_t`].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub extern "C" fn wasmer_target_delete(_target: Option<Box<wasmer_target_t>>) {}

/// Unstable non-standard Wasmer-specific API to represent a target
/// “triple”.
///
/// Historically such things had three fields, though they have added
/// additional fields over time.
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
///     wasm_name_t triple_name;
///     wasm_name_new_from_string(&triple_name, "x86_64-apple-darwin");
///
///     wasmer_triple_t* triple = wasmer_triple_new(&triple_name);
///     assert(triple);
///
///     wasmer_triple_delete(triple);
///     wasm_name_delete(&triple_name);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// See also [`wasmer_triple_new_from_host`].
#[allow(non_camel_case_types)]
pub struct wasmer_triple_t {
    inner: Triple,
}

/// Create a new [`wasmer_triple_t`] based on a triple string.
///
/// # Example
///
/// See [`wasmer_triple_t`] or [`wasmer_triple_new_from_host`].
#[no_mangle]
pub unsafe extern "C" fn wasmer_triple_new(
    triple: Option<&wasm_name_t>,
) -> Option<Box<wasmer_triple_t>> {
    let triple = triple?;
    let triple = c_try!(str::from_utf8(slice::from_raw_parts(
        triple.data,
        triple.size
    )));

    Some(Box::new(wasmer_triple_t {
        inner: c_try!(Triple::from_str(triple)),
    }))
}

/// Create the [`wasmer_triple_t`] for the current host.
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
///     wasmer_triple_t* triple = wasmer_triple_new_from_host();
///     assert(triple);
///
///     wasmer_triple_delete(triple);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
///
/// See also [`wasmer_triple_new`].
#[no_mangle]
pub extern "C" fn wasmer_triple_new_from_host() -> Box<wasmer_triple_t> {
    Box::new(wasmer_triple_t {
        inner: Triple::host(),
    })
}

/// Delete a [`wasmer_triple_t`].
///
/// # Example
///
/// See [`wasmer_triple_t`].
#[no_mangle]
pub extern "C" fn wasmer_triple_delete(_triple: Option<Box<wasmer_triple_t>>) {}

/// Unstable non-standard Wasmer-specific API to represent a set of
/// CPU features.
///
/// CPU features are identified by their stringified names. The
/// reference is the GCC options:
///
/// * <https://gcc.gnu.org/onlinedocs/gcc/x86-Options.html>,
/// * <https://gcc.gnu.org/onlinedocs/gcc/ARM-Options.html>,
/// * <https://gcc.gnu.org/onlinedocs/gcc/AArch64-Options.html>.
///
/// At the time of writing this documentation (it might be outdated in
/// the future), the supported features are the following:
///
/// * `sse2`,
/// * `sse3`,
/// * `ssse3`,
/// * `sse4.1`,
/// * `sse4.2`,
/// * `popcnt`,
/// * `avx`,
/// * `bmi`,
/// * `bmi2`,
/// * `avx2`,
/// * `avx512dq`,
/// * `avx512vl`,
/// * `lzcnt`.
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
///     // Create a new CPU feature set.
///     wasmer_cpu_features_t* cpu_features = wasmer_cpu_features_new();
///
///     // Create a new feature name, here `sse2`, and add it to the set.
///     {
///         wasm_name_t cpu_feature_name;
///         wasm_name_new_from_string(&cpu_feature_name, "sse2");
///
///         wasmer_cpu_features_add(cpu_features, &cpu_feature_name);
///
///         wasm_name_delete(&cpu_feature_name);
///     }
///
///     wasmer_cpu_features_delete(cpu_features);
///
///     return 0;
/// }
/// #    })
/// #    .success();
/// # }
/// ```
#[allow(non_camel_case_types)]
pub struct wasmer_cpu_features_t {
    inner: EnumSet<CpuFeature>,
}

/// Create a new [`wasmer_cpu_features_t`].
///
/// # Example
///
/// See [`wasmer_cpu_features_t`].
#[no_mangle]
pub extern "C" fn wasmer_cpu_features_new() -> Box<wasmer_cpu_features_t> {
    Box::new(wasmer_cpu_features_t {
        inner: CpuFeature::set(),
    })
}

/// Delete a [`wasmer_cpu_features_t`].
///
/// # Example
///
/// See [`wasmer_cpu_features_t`].
#[no_mangle]
pub extern "C" fn wasmer_cpu_features_delete(_cpu_features: Option<Box<wasmer_cpu_features_t>>) {}

/// Add a new CPU feature into the set represented by
/// [`wasmer_cpu_features_t`].
///
/// # Example
///
/// See [`wasmer_cpu_features_t`].
#[no_mangle]
pub unsafe extern "C" fn wasmer_cpu_features_add(
    cpu_features: Option<&mut wasmer_cpu_features_t>,
    feature: Option<&wasm_name_t>,
) -> bool {
    let cpu_features = match cpu_features {
        Some(cpu_features) => cpu_features,
        _ => return false,
    };
    let feature = match feature {
        Some(feature) => feature,
        _ => return false,
    };
    let feature = c_try!(
        str::from_utf8(slice::from_raw_parts(
            feature.data,
            feature.size,
        ));
        otherwise false
    );

    cpu_features.inner.insert(c_try!(
        CpuFeature::from_str(feature);
        otherwise false
    ));

    true
}
