use super::*;
use crate::syscalls::*;
use wasmer::Type;
use wasmer::ValueType;

/// Call a function from the `__indirect_function_table` with parameters and results from memory.
///
/// This function can be used to call functions whose types are not known at
/// compile time of the caller. It is the callers responsibility to ensure
/// that the passed parameters and results match the signature of the function
/// being called.
///
/// ### Parameters
///
/// * function_id: The indirect function table index of the function to call
///
/// * values: Pointer to a sequence of values that will be passed to the function.
///   If the function does not have any parameters, this can be a nullptr (0).
///
/// * results: Pointer to a sequence of values.
///   If the function does not return a value, this can be a nullptr (0).
///   The buffer needs to be large enough to hold all return values.
///
/// * strict: if true, the values must match the function's input count and
///   types exactly, and the results buffer must be exactly as large as the
///   number of return values from the function. If false, missing values will
///   be filled with zeroes/defaults, and extra values will be ignored.
///
#[instrument(
    level = "trace",
    skip_all,
    fields(%function_id, values_ptr = values.offset().into(), results_ptr = results.offset().into()),
    ret
)]
#[allow(clippy::result_large_err)]
pub fn call_dynamic2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    values: WasmPtr<WasmRawValueWithType, M>,
    values_len: M::Offset,
    results: WasmPtr<WasmRawValueWithType, M>,
    results_len_ptr: WasmPtr<M::Offset, M>,
    strict: Bool,
) -> Result<Errno, RuntimeError> {
    let (env, mut store) = ctx.data_and_store_mut();

    let strict = matches!(strict, Bool::True);

    let function = wasi_try_ok!(
        env.inner()
            .indirect_function_table_lookup(&mut store, function_id)
            .map_err(Errno::from)
    );

    let memory = unsafe { env.memory_view(&store) };

    tracing::debug!("1");
    let values = wasi_try_ok!(
        values
            .slice(&memory, values_len)
            .and_then(|s| s.read_to_vec())
            .map_err(mem_error_to_wasi)
    );

    // There is no reflection in JS, so we cannot perform strict checks there
    tracing::debug!("2");
    let mut is_js = false;
    #[cfg(feature = "js")]
    {
        is_js = store.engine().is_js();
    }
    if strict && !is_js {
        let ty = function.ty(&store);
        if ty.params().len() != values.len() {
            return Ok(Errno::Inval);
        }
        for (i, param_ty) in ty.params().iter().enumerate() {
            if Type::from(values[i].type_) != *param_ty {
                return Ok(Errno::Inval);
            }
        }
    }

    tracing::debug!("3");
    let values = values
        .into_iter()
        .map(|v| match v.type_ {
            WasmValueType::I32 => {
                Value::I32(i32::from_le_bytes(v.value.bytes[0..4].try_into().unwrap()))
            }
            WasmValueType::I64 => {
                Value::I64(i64::from_le_bytes(v.value.bytes[0..8].try_into().unwrap()))
            }
            WasmValueType::F32 => {
                Value::F32(f32::from_le_bytes(v.value.bytes[0..4].try_into().unwrap()))
            }
            WasmValueType::F64 => {
                Value::F64(f64::from_le_bytes(v.value.bytes[0..8].try_into().unwrap()))
            }
            WasmValueType::V128 => Value::V128(u128::from_le_bytes(
                v.value.bytes[0..16].try_into().unwrap(),
            )),
        })
        .collect::<Vec<_>>();

    tracing::debug!("4");
    let result_values = function
        .call(&mut store, values.as_slice())
        .map_err(crate::flatten_runtime_error)?;

    tracing::debug!("5");
    let memory = unsafe { env.memory_view(&store) };
    let results_buffer_len =
        wasi_try_ok!(results_len_ptr.read(&memory).map_err(mem_error_to_wasi)).into() as usize;

    tracing::debug!("6");
    if strict {
        if result_values.len() != results_buffer_len {
            return Ok(Errno::Inval);
        }
    } else {
        tracing::debug!("7");
        wasi_try_ok!(
            results_len_ptr
                .write(&memory, M::Offset::from(result_values.len() as u32))
                .map_err(mem_error_to_wasi)
        );
    }

    tracing::debug!("8");
    for i in 0..results_buffer_len {
        let mut buffer = vec![0u8; 16];
        let type_ = match result_values.get(i) {
            None => WasmValueType::I32,
            Some(Value::I32(n)) => {
                buffer[0..4].copy_from_slice(&n.to_le_bytes());
                WasmValueType::I32
            }
            Some(Value::I64(n)) => {
                buffer[0..8].copy_from_slice(&n.to_le_bytes());
                WasmValueType::I64
            }
            Some(Value::F32(n)) => {
                buffer[0..4].copy_from_slice(&n.to_le_bytes());
                WasmValueType::F32
            }
            Some(Value::F64(n)) => {
                buffer[0..8].copy_from_slice(&n.to_le_bytes());
                WasmValueType::F64
            }
            Some(Value::V128(n)) => {
                buffer[0..16].copy_from_slice(&n.to_le_bytes());
                WasmValueType::V128
            }
            _ => {
                return Ok(Errno::Inval);
            }
        };
        tracing::debug!("9");
        let raw_value = WasmRawValueWithType {
            type_,
            value: WasmRawValue {
                bytes: buffer.try_into().unwrap(),
            },
        };
        tracing::debug!("10");
        wasi_try_ok!(
            results
                .add_offset(M::Offset::from(i as u32))
                .and_then(|ptr| ptr.write(&memory, raw_value))
                .map_err(mem_error_to_wasi)
        );
    }

    tracing::debug!("11");
    Ok(Errno::Success)
}
