//! Contains everything to create a target with a triple and CPU featurres.
//!
//! # Example
//!
//! ```rust
//! # use inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer_wasm.h"
//! #
//! int main() {
//!     wasm_byte_vec_t triple_name;
//!     wasmer_byte_vec_new_from_string(&triple_name, "x86_64-apple-darwin");
//!
//!     wasm_triple_t* triple = wasm_triple_new((wasm_name_t*) &triple_name);
//!     assert(triple);
//!
//!     wasm_byte_vec_t cpu_feature_name;
//!     wasmer_byte_vec_new_from_string(&cpu_feature_name, "sse2");
//!
//!     wasm_cpu_features_t* cpu_features = wasm_cpu_features_new();
//!     wasm_cpu_features_add(cpu_features, (wasm_name_t*) &cpu_feature_name);
//!
//!     wasm_target_t* target = wasm_target_new(triple, cpu_features);
//!     assert(target);
//!
//!     wasm_target_delete(target);
//!     wasm_byte_vec_delete(&cpu_feature_name);
//!     wasm_byte_vec_delete(&triple_name);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```

use super::super::types::wasm_name_t;
use crate::error::CApiError;
use enumset::EnumSet;
use std::ffi::CStr;
use std::slice;
use std::str::FromStr;
use wasmer_compiler::{CpuFeature, Target, Triple};

/// Represents a triple + CPU features pair.
#[allow(non_camel_case_types)]
pub struct wasm_target_t {
    pub(crate) inner: Target,
}

/// Createas a new `wasm_target_`.
///
/// It takes ownership of `triple` and `cpu_features`.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_target_new(
    triple: Option<Box<wasm_triple_t>>,
    cpu_features: Option<Box<wasm_cpu_features_t>>,
) -> Option<Box<wasm_target_t>> {
    let triple = triple?;
    let cpu_features = cpu_features?;

    Some(Box::new(wasm_target_t {
        inner: Target::new(triple.inner.clone(), cpu_features.inner.clone()),
    }))
}

/// Delete a `wasm_target_t`.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_target_delete(_target: Option<Box<wasm_target_t>>) {}

#[allow(non_camel_case_types)]
pub struct wasm_triple_t {
    inner: Triple,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_triple_new(
    triple: Option<&wasm_name_t>,
) -> Option<Box<wasm_triple_t>> {
    let triple = triple?;
    let triple = c_try!(CStr::from_bytes_with_nul_unchecked(slice::from_raw_parts(
        triple.data,
        triple.size + 1
    ))
    .to_str());

    Some(Box::new(wasm_triple_t {
        inner: c_try!(Triple::from_str(triple).map_err(|e| CApiError { msg: e.to_string() })),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_triple_new_from_host() -> Box<wasm_triple_t> {
    Box::new(wasm_triple_t {
        inner: Triple::host(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_triple_delete(_triple: Option<Box<wasm_triple_t>>) {}

#[allow(non_camel_case_types)]
pub struct wasm_cpu_features_t {
    inner: EnumSet<CpuFeature>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_cpu_features_new() -> Box<wasm_cpu_features_t> {
    Box::new(wasm_cpu_features_t {
        inner: CpuFeature::set(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_cpu_features_delete(_cpu_features: Option<Box<wasm_cpu_features_t>>) {
}

#[no_mangle]
pub unsafe extern "C" fn wasm_cpu_features_add(
    cpu_features: Option<&mut wasm_cpu_features_t>,
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
        CStr::from_bytes_with_nul_unchecked(slice::from_raw_parts(
            feature.data,
            feature.size + 1,
        ))
            .to_str();
        otherwise false
    );

    cpu_features.inner.insert(c_try!(
        CpuFeature::from_str(feature);
        otherwise false
    ));

    true
}
