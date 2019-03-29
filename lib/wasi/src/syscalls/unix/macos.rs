use crate::syscalls::types::*;
use crate::ptr::{Array, WasmPtr},
use wasmer_runtime_core::{memory::Memory, vm::Ctx};

pub fn platform_clock_res_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    __WASI_EINVAL
}

pub fn platform_clock_time_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
