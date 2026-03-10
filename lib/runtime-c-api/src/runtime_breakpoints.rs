use crate::instance::wasmer_instance_t;

use wasmer_middleware_common::runtime_breakpoints::{
    set_runtime_breakpoint_value,
    get_runtime_breakpoint_value,
    BREAKPOINT_VALUE_NO_BREAKPOINT
};

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_set_runtime_breakpoint_value(
    instance: *mut wasmer_instance_t,
    value: u64,
) {
    if instance.is_null() {
        return;
    }
    let instance = &mut *(instance as *mut wasmer_runtime::Instance);
    set_runtime_breakpoint_value(instance, value);
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_get_runtime_breakpoint_value(
    instance: *mut wasmer_instance_t,
) -> u64 {
    if instance.is_null() {
        return BREAKPOINT_VALUE_NO_BREAKPOINT;
    }
    let instance = &mut *(instance as *mut wasmer_runtime::Instance);

    get_runtime_breakpoint_value(instance)
}
