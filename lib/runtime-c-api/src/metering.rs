use crate::{
    error::{update_last_error, CApiError},
    instance::wasmer_instance_t,
    module::wasmer_module_t,
    wasmer_result_t,
};
use std::slice;

#[cfg(feature = "metering")]
use wasmer_runtime_core::backend::Compiler;

#[cfg(not(feature = "cranelift-backend"))]
use wasmer_middleware_common::metering;

pub const OPCODE_COUNT: usize = 448;
pub static mut OPCODE_COSTS: [u32; OPCODE_COUNT] = [0; OPCODE_COUNT];

#[allow(clippy::cast_ptr_alignment)]
#[cfg(feature = "metering")]
#[no_mangle]
pub unsafe extern "C" fn wasmer_set_opcode_costs(
    opcode_costs_pointer: *const u32,
) {
    OPCODE_COSTS.copy_from_slice(slice::from_raw_parts(opcode_costs_pointer, OPCODE_COUNT));
}


// returns gas used
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
#[cfg(feature = "metering")]
pub unsafe extern "C" fn wasmer_instance_get_points_used(instance: *mut wasmer_instance_t) -> u64 {
    if instance.is_null() {
        return 0;
    }
    let instance = &*(instance as *const wasmer_runtime::Instance);
    let points = metering::get_points_used(instance);
    points
}

// sets gas used
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
#[cfg(feature = "metering")]
pub unsafe extern "C" fn wasmer_instance_set_points_used(
    instance: *mut wasmer_instance_t,
    new_gas: u64,
) {
    if instance.is_null() {
        return;
    }
    let instance = &mut *(instance as *mut wasmer_runtime::Instance);
    metering::set_points_used(instance, new_gas)
}

// sets gas limit
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
#[cfg(feature = "metering")]
pub unsafe extern "C" fn wasmer_instance_set_points_limit(
    instance: *mut wasmer_instance_t,
    limit: u64,
) {
    if instance.is_null() {
        return;
    }
    let instance = &mut *(instance as *mut wasmer_runtime::Instance);
    metering::set_points_limit(instance, limit)
}


/// Creates a new Module with gas limit from the given wasm bytes.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[cfg(feature = "metering")]
#[no_mangle]
pub unsafe extern "C" fn wasmer_compile_with_gas_metering(
    module: *mut *mut wasmer_module_t,
    wasm_bytes: *mut u8,
    wasm_bytes_len: u32,
) -> wasmer_result_t {
    if module.is_null() {
        update_last_error(CApiError {
            msg: "module is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
    if wasm_bytes.is_null() {
        update_last_error(CApiError {
            msg: "wasm bytes is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let compiler = get_metered_compiler();

    let bytes: &[u8] = slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize);
    let result = wasmer_runtime_core::compile_with(bytes, &compiler);
    let new_module = match result {
        Ok(instance) => instance,
        Err(_) => {
            update_last_error(CApiError {
                msg: "compile error".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *module = Box::into_raw(Box::new(new_module)) as *mut wasmer_module_t;
    wasmer_result_t::WASMER_OK
}

#[cfg(feature = "metering")]
unsafe fn get_metered_compiler() -> impl Compiler {
    use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};

    #[cfg(feature = "llvm-backend")]
    use wasmer_llvm_backend::ModuleCodeGenerator as MeteredMCG;

    #[cfg(feature = "singlepass-backend")]
    use wasmer_singlepass_backend::ModuleCodeGenerator as MeteredMCG;

    #[cfg(feature = "cranelift-backend")]
    use wasmer_clif_backend::CraneliftModuleCodeGenerator as MeteredMCG;

    use wasmer_middleware_common::runtime_breakpoints;

    let c: StreamingCompiler<MeteredMCG, _, _, _, _> = StreamingCompiler::new(move || {
        let mut chain = MiddlewareChain::new();

        chain.push(metering::Metering::new(&OPCODE_COSTS, 0));
        chain.push(runtime_breakpoints::RuntimeBreakpointHandler::new());

        chain
    });
    c
}


/*** placeholder implementation if metering feature off ***/

// Without metering, wasmer_compile_with_gas_metering is a copy of wasmer_compile
#[cfg(not(feature = "metering"))]
#[no_mangle]
pub unsafe extern "C" fn wasmer_compile_with_gas_metering(
    module: *mut *mut wasmer_module_t,
    wasm_bytes: *mut u8,
    wasm_bytes_len: u32,
) -> wasmer_result_t {
    if module.is_null() {
        update_last_error(CApiError {
            msg: "module is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
    if wasm_bytes.is_null() {
        update_last_error(CApiError {
            msg: "wasm bytes is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let bytes: &[u8] = slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize);
    // TODO: this implicitly uses default_compiler() is that proper? maybe we override default_compiler
    let result = wasmer_runtime::compile(bytes);
    let new_module = match result {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *module = Box::into_raw(Box::new(new_module)) as *mut wasmer_module_t;
    wasmer_result_t::WASMER_OK
}

// returns gas used -- placeholder implementation, when "metering" is disabled
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
#[cfg(not(feature = "metering"))]
pub unsafe extern "C" fn wasmer_instance_get_points_used(_: *mut wasmer_instance_t) -> u64 {
    0
}

// sets gas used -- placeholder implementation, when "metering" is disabled
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
#[cfg(not(feature = "metering"))]
pub unsafe extern "C" fn wasmer_instance_set_points_used(_: *mut wasmer_instance_t, _: u64) {}
