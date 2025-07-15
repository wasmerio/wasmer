use super::*;
use crate::syscalls::*;
use wasmer::Type;
use wasmer::ValueType;

macro_rules! write_value {
    ($memory:expr, $offset:expr, $max:expr, $strict:expr, $value:expr) => {{
        let bytes = $value.to_le_bytes();
        if $offset + bytes.len() as u64 <= $max {
            $memory.write($offset, &bytes)?;
            $offset += bytes.len() as u64;
            Ok(true)
        } else {
            Ok(!$strict)
        }
    }};
}

fn write_value(
    memory: &MemoryView,
    offset: &mut u64,
    max: u64,
    strict: bool,
    value: &Value,
) -> Result<bool, MemoryAccessError> {
    match value {
        Value::I32(value) => write_value!(memory, *offset, max, strict, value),
        Value::I64(value) => write_value!(memory, *offset, max, strict, value),
        Value::F32(value) => write_value!(memory, *offset, max, strict, value),
        Value::F64(value) => write_value!(memory, *offset, max, strict, value),
        Value::V128(value) => write_value!(memory, *offset, max, strict, value),
        // ExternRef, FuncRef, and ExceptionRef cannot be represented as byte slices
        _ => panic!("Cannot write non-scalar value as bytes"),
    }
}

macro_rules! read_value {
    ($memory:expr, $offset:expr, $max:expr, $strict:expr, $ty:ident, $val:ident, $len:expr) => {{
        if $offset + $len > $max {
            Ok(if $strict {
                None
            } else {
                Some(Value::$val($ty::default()))
            })
        } else {
            let mut buffer = [0u8; $len];
            $memory.read($offset, &mut buffer)?;
            $offset += $len;
            Ok(Some(Value::$val($ty::from_le_bytes(buffer))))
        }
    }};
}

fn read_value(
    memory: &MemoryView,
    offset: &mut u64,
    max: u64,
    strict: bool,
    ty: &Type,
) -> Result<Option<Value>, MemoryAccessError> {
    match ty {
        Type::I32 => read_value!(memory, *offset, max, strict, i32, I32, 4),
        Type::I64 => read_value!(memory, *offset, max, strict, i64, I64, 8),
        Type::F32 => read_value!(memory, *offset, max, strict, f32, F32, 4),
        Type::F64 => read_value!(memory, *offset, max, strict, f64, F64, 8),
        Type::V128 => read_value!(memory, *offset, max, strict, u128, V128, 16),
        // ExternRef, FuncRef, and ExceptionRef cannot be represented as byte slices
        _ => panic!("Cannot read non-scalar value from memory"),
    }
}

/// Call a function from the `__indirect_function_table` with parameters and results from memory.
///
/// This function can be used to call functions whose types are not known at
/// compile time of the caller. It is the callers responsibility to ensure
/// that the passed parameters and results match the signature of the function
/// beeing called.
///
/// ### Format of the values and results buffer
///
/// The buffers contain all values sequentially. i32, and f32 are 4 bytes,
/// i64 and f64 are 8 bytes, v128 is 16 bytes.
///     
/// For example if the function takes an i32 and an i64, the values buffer will
/// be 12 bytes long, with the first 4 bytes being the i32 and the next 8
/// bytes being the i64.
///
/// ### Parameters
///
/// * function_id: The indirect function table index of the function to call
///
/// * values: Pointer to a sequence of values that will be passed to the function.
///   The buffer will be interpreted as described above.
///   If the function does not have any parameters, this can be a nullptr (0).
///
/// * results: Pointer to a sequence of values.
///   If the function does not return a value, this can be a nullptr (0).
///   The buffer needs to be large enough to hold all return values.
///
#[instrument(
    level = "trace",
    skip_all,
    fields(%function_id, values_ptr = values.offset().into(), results_ptr = results.offset().into()),
    ret
)]
#[allow(clippy::result_large_err)]
pub fn call_dynamic<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    values: WasmPtr<u8, M>,
    values_len: M::Offset,
    results: WasmPtr<u8, M>,
    results_len: M::Offset,
    strict: Bool,
) -> Result<Errno, WasiRuntimeError> {
    let (env, mut store) = ctx.data_and_store_mut();

    let strict = matches!(strict, Bool::True);

    let function = wasi_try_ok!(env
        .inner()
        .indirect_function_table_lookup(&mut store, function_id)
        .and_then(|f| f.ok_or(Errno::Inval)));

    let function_type = function.ty(&store);

    let memory = unsafe { env.memory_view(&store) };
    let mut current_values_offset: u64 = values.offset().into();
    let max_values_offset = current_values_offset + values_len.into();
    let mut values_buffer = vec![];
    for ty in function_type.params() {
        let Some(value) = wasi_try_mem_ok!(read_value(
            &memory,
            &mut current_values_offset,
            max_values_offset,
            strict,
            ty
        )) else {
            return Ok(Errno::Inval);
        };
        values_buffer.push(value);
    }

    if strict && current_values_offset != max_values_offset {
        // If strict is true, we expect to have read all values
        return Ok(Errno::Inval);
    }

    let result_values = function.call(&mut store, values_buffer.as_slice())?;

    let memory = unsafe { env.memory_view(&store) };
    let mut current_results_offset: u64 = results.offset().into();
    let max_results_offset = current_results_offset + results_len.into();
    for result_value in result_values {
        wasi_try_mem_ok!(write_value(
            &memory,
            &mut current_results_offset,
            max_results_offset,
            strict,
            &result_value
        ));
    }

    if strict && current_results_offset != max_results_offset {
        // If strict is true, we expect to have written all results
        return Ok(Errno::Inval);
    }

    Ok(Errno::Success)
}
