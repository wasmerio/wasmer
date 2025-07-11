use super::*;
use crate::syscalls::*;
use wasmer::Type;

// TODO: Decide on whether to move this back into Value or not.
#[cfg(target_endian = "little")]
/// Get a slice to the content of this value if it is a scalar type.
///
/// Returns `None` for Value that can not be freely shared between contexts.
/// Returns `None` if the value is not representable as a byte slice.
///
/// Not available on big-endian architectures, because the result of this function
/// should be compatible with wasm memory, which is little-endian.
fn value_as_bytes(value: &Value) -> Option<&[u8]> {
    match value {
        Value::I32(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts(value as *const i32 as *const u8, 4) })
        }
        Value::I64(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts(value as *const i64 as *const u8, 8) })
        }
        Value::F32(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts(value as *const f32 as *const u8, 4) })
        }
        Value::F64(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts(value as *const f64 as *const u8, 8) })
        }
        Value::V128(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts(value as *const u128 as *const u8, 16) })
        }
        // ExternRef, FuncRef, and ExceptionRef cannot be represented as byte slices
        _ => None,
    }
}

// TODO: Decide on whether to move this back into Value or not.
#[cfg(target_endian = "little")]
/// Get a mutable slice to the content of this value if it is a scalar type.
///
/// Returns `None` for Value that can not be freely shared between contexts.
/// Returns `None` if the value is not representable as a byte slice.
///
/// Not available on big-endian architectures, because the result of this function
/// should be compatible with wasm memory, which is little-endian.
fn value_as_bytes_mut(value: &mut Value) -> Option<&mut [u8]> {
    match value {
        Value::I32(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts_mut(value as *mut i32 as *mut u8, 4) })
        }
        Value::I64(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts_mut(value as *mut i64 as *mut u8, 8) })
        }
        Value::F32(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts_mut(value as *mut f32 as *mut u8, 4) })
        }
        Value::F64(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts_mut(value as *mut f64 as *mut u8, 8) })
        }
        Value::V128(value) => {
            // Safety: This function is only enabled on little-endian architectures,
            Some(unsafe { std::slice::from_raw_parts_mut(value as *mut u128 as *mut u8, 16) })
        }
        // ExternRef, FuncRef, and ExceptionRef cannot be represented as byte slices
        _ => None,
    }
}

/// Call a function from the `__indirect_function_table` with parameters and results from memory.
///
/// This function can be used to call functions whose types are not known at compile time of the caller. It is the callers responsibility to ensure that the passed parameters and results match the signature of the function beeing called.
///
/// ### Format of the values and results buffer
///
/// The buffers contain all values sequentially. i32, and f32 are 4 bytes, i64 and f64 are 8 bytes, v128 is 16 bytes.
///     
/// For example if the function takes an i32 and an i64, the values buffer will be 12 bytes long, with the first 4 bytes being the i32 and the next 8 bytes being the i64.
///
/// ### Parameters
///
/// * function_id: The indirect function table index of the function to call
///
/// * values: Pointer to a sequence of values that will be passed to the function.
///   
///   The buffer will be interpreted as described above
///
///   If the function does not have any parameters, this can be a nullptr (0).
///
/// * results: Pointer to a sequence of values
///   
///   If the function does not return a value, this can be a nullptr (0).
///
///   The buffer needs to be large enough to hold all return values.
///
#[instrument(level = "trace", skip_all, fields(%function_id, values_ptr = values.offset().into(), results_ptr = results.offset().into()), ret)]
pub fn call_dynamic<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    values: WasmPtr<u8, M>,
    results: WasmPtr<u8, M>,
) -> Result<Errno, WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let function = wasi_try_ok!(env
        .inner()
        .main_module_indirect_function_table_lookup(&mut store, function_id));

    let function_type = function.ty(&store);

    let memory = unsafe { env.memory_view(&store) };
    let mut current_values_offset: u64 = values.offset().into();
    let values_buffer = function_type
        .params()
        .iter()
        .map(|ty| {
            let mut value = Value::default_typed(ty); // Initialize a default value for the type
            let buffer = value_as_bytes_mut(&mut value).unwrap(); // This should never fail, because a function's parameters are always valid types
            memory
                .read(current_values_offset, buffer)
                .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
            current_values_offset += buffer.len() as u64; // Move to the next value offset
            Ok(value)
        })
        .collect::<Result<Vec<_>, WasiRuntimeError>>()?;

    let result_values = function.call(&mut store, values_buffer.as_slice())?;

    let memory = unsafe { env.memory_view(&store) };
    let mut current_results_offset: u64 = results.offset().into();
    result_values.iter().try_for_each(|result_value| {
        let bytes = value_as_bytes(result_value).unwrap();
        memory
            .write(current_results_offset, &bytes)
            .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
        current_results_offset += bytes.len() as u64; // Move to the next result offset
        Result::<(), WasiRuntimeError>::Ok(())
    })?;

    Ok((Errno::Success))
}
