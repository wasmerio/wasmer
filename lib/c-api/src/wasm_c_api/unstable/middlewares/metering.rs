//! Unstable non-standard Wasmer-specific API that contains everything
//! to create a the middleware metering API.
//!
//! # Example
//!
//! ```rust
//! # use inline_c::assert_c;
//! # fn main() {
//! #    (assert_c! {
//! # #include "tests/wasmer_wasm.h"
//! #
//! int main() {//!//!     
//!     wasmer_metering_t* metering = wasmer_metering_new(10);
//!     wasmer_module_middleware_t* middleware = wasmer_metering_as_middleware(metering);
//!     
//!     wasm_config_t* config = wasm_config_new();
//!     wasm_config_push_middleware(config, middleware);
//!     
//!     wasm_engine_t* engine = wasm_engin_new_with_config(config);
//!
//!     wasm_store_t* store = wasm_store_new(engine);
//!     
//!     wasm_byte_vec_t wat;
//!     wasmer_byte_vec_new_from_string(
//!         &wat,
//!         "(module\n"
//!         "  (type $add_t (func (param i32) (result i32)))\n"
//!         "  (func $add_one_f (type $add_t) (param $value i32) (result i32)\n"
//!         "    local.get $value\n"
//!         "    i32.const 1\n"
//!         "    i32.add)\n"
//!         "  (export "add_one" (func $add_one_f)))"
//!     );
//!     wasm_byte_vec_t wasm;
//!     wat2wasm(&wat, &wasm);
//!
//!     wasm_module_t* module = wasm_module_new(store, &wasm);
//!     assert(module);
//!     
//!     wasm_extern_vec_t imports = WASM_EMPTY_VEC;
//!     wasm_trap_t* traps = NULL;
//!     wasm_instance_t* instance = wasm_instance_new(store, module, &imports, &traps);
//!     assert(instance);
//!     
//!     
//!     
//!     
//!     wasm_instance_delete(instance);
//!     wasm_module_delete(module);
//!     wasm_store_delete(store);
//!     wasm_engine_delete(engine);
//!
//!     return 0;
//! }
//! #    })
//! #    .success();
//! # }
//! ```

use super::super::super::engine::wasmer_module_middleware_t;
use super::super::super::instance::wasm_instance_t;
use std::sync::Arc;
use wasmer::wasmparser::Operator;
use wasmer_middlewares::{
    metering::{get_remaining_points, set_remaining_points, MeteringPoints},
    Metering,
};

/// Opaque type representing a MeteringPoints.
#[allow(non_camel_case_types)]
pub struct wasmer_metering_points_t {
    pub(crate) inner: MeteringPoints,
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_points_delete(
    _metering_points: Option<Box<wasmer_metering_points_t>>,
) {
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_points_unwrap_or(
    metering_points: &wasmer_metering_points_t,
    exhausted: u64,
) -> u64 {
    match metering_points.inner {
        MeteringPoints::Remaining(value) => value,
        MeteringPoints::Exhausted => exhausted,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_points_is_exhausted(
    metering_points: &wasmer_metering_points_t,
) -> bool {
    matches!(metering_points.inner, MeteringPoints::Exhausted)
}

/// Opaque type representing a MeteringPoints.
#[allow(non_camel_case_types)]
pub struct wasmer_metering_t {
    #[allow(dead_code)]
    pub(crate) inner: Arc<Metering<fn(&Operator) -> u64>>,
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_new(initial_limit: u64) -> Box<wasmer_metering_t> {
    let cost_function = |operator: &Operator| -> u64 {
        match operator {
            Operator::I32Const { .. }
            | Operator::I64Const { .. }
            | Operator::F32Const { .. }
            | Operator::F64Const { .. } => 0,
            _ => 1,
        }
    };
    Box::new(wasmer_metering_t {
        inner: Arc::new(Metering::new(initial_limit, cost_function)),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_get_remaining_points(
    instance: &wasm_instance_t,
) -> Box<wasmer_metering_points_t> {
    Box::new(wasmer_metering_points_t {
        inner: get_remaining_points(&instance.inner),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_set_remaining_points(
    instance: &wasm_instance_t,
    new_limit: u64,
) {
    set_remaining_points(&instance.inner, new_limit);
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_as_middleware(
    metering: Option<Box<wasmer_metering_t>>,
) -> Option<Box<wasmer_module_middleware_t>> {
    let metering = metering?;

    Some(Box::new(wasmer_module_middleware_t {
        inner: metering.inner,
    }))
}
