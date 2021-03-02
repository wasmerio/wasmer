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
//! int main() {
//!     wasmer_metering_t* metering = wasmer_metering_new(10);
//!     wasmer_module_middleware_t* middleware = wasmer_metering_as_middleware(metering);
//!     
//!     wasm_config_t* config = wasm_config_new();
//!     wasm_config_push_middleware(config, middleware);
//!     
//!     wasm_engine_t* engine = wasm_engine_new_with_config(config);
//!
//!     wasm_store_t* store = wasm_store_new(engine);
//!     
//!     wasm_byte_vec_t wat;
//!     wasmer_byte_vec_new_from_string(
//!         &wat,
//!         "(module\n"
//!         "  (type $add_t (func (param i32) (result i32)))\n"
//!         "  (func $add_two_f (type $add_t) (param $value i32) (result i32)\n"
//!         "    local.get $value\n"
//!         "    i32.const 1\n"
//!         "    i32.add\n"
//!         "    i32.const 1\n"
//!         "    i32.add)\n"
//!         "  (export \"add_two\" (func $add_two_f)))"
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
//!     wasm_extern_vec_t exports;
//!     wasm_instance_exports(instance, &exports);
//!     assert(exports.size >= 1);
//!     assert(wasm_extern_kind(exports.data[0]) == WASM_EXTERN_FUNC);
//!
//!     const wasm_func_t* add_two = wasm_extern_as_func(exports.data[0]);
//!     assert(add_two);
//!
//!     wasm_val_t arguments[1] = { WASM_I32_VAL(40) };
//!     wasm_val_t results[1] = { WASM_INIT_VAL };
//!
//!     wasm_val_vec_t arguments_as_array = WASM_ARRAY_VEC(arguments);
//!     wasm_val_vec_t results_as_array = WASM_ARRAY_VEC(results);
//!
//!     uint64_t exhausted_value = -1;
//!
//!     {
//!         wasm_trap_t* trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         assert(trap == NULL);
//!         assert(results[0].of.i32 == 42);
//!
//!         wasmer_metering_points_t* metering_points = wasmer_metering_get_remaining_points(instance);
//!         assert(wasmer_metering_points_unwrap_or(metering_points, exhausted_value) == 6);
//!         assert(wasmer_metering_points_is_exhausted(metering_points) == false);
//!         wasmer_metering_points_delete(metering_points);
//!     }
//!
//!     {
//!         wasm_trap_t* trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         assert(trap == NULL);
//!         assert(results[0].of.i32 == 42);
//!
//!         wasmer_metering_points_t* metering_points = wasmer_metering_get_remaining_points(instance);
//!         assert(wasmer_metering_points_unwrap_or(metering_points, exhausted_value) == 2);
//!         assert(wasmer_metering_points_is_exhausted(metering_points) == false);
//!         wasmer_metering_points_delete(metering_points);
//!     }
//!
//!     {
//!         wasm_trap_t* trap = wasm_func_call(add_two, &arguments_as_array, &results_as_array);
//!         assert(trap != NULL);
//!
//!         wasmer_metering_points_t* metering_points = wasmer_metering_get_remaining_points(instance);
//!         assert(wasmer_metering_points_unwrap_or(metering_points, exhausted_value) == exhausted_value);
//!         assert(wasmer_metering_points_is_exhausted(metering_points) == true);
//!         wasmer_metering_points_delete(metering_points);
//!     }
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

use super::super::super::instance::wasm_instance_t;
use super::wasmer_middleware_t;
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
pub unsafe extern "C" fn wasmer_metering_delete(_metering: Option<Box<wasmer_metering_t>>) {}

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
) -> Option<Box<wasmer_middleware_t>> {
    let metering = metering?;

    Some(Box::new(wasmer_middleware_t {
        inner: metering.inner,
    }))
}
