// C API for metering.

use super::wasm_c_api::instance::wasm_instance_t;
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
pub unsafe extern "C" fn wasmer_metering_points_value(
    metering_points: &Box<wasmer_metering_points_t>,
    exhausted: u64,
) -> u64 {
    match metering_points.inner {
        MeteringPoints::Remaining(value) => value,
        MeteringPoints::Exhausted => exhausted,
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_metering_points_is_exhausted(
    metering_points: &Box<wasmer_metering_points_t>,
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
