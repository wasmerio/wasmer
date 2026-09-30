use super::*;
use crate::syscalls::*;
use wasmer::AsyncFunctionEnvMut;
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

/// The work either flavour of `call_dynamic` does before re-entering the guest:
/// resolve the table entry and decode the parameters out of guest memory.
enum Prepared {
    /// The resolved function and its decoded arguments.
    Call(wasmer::Function, Vec<Value>),
    /// The call was rejected before it started.
    Fail(Errno),
}

fn prepare<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    values: WasmPtr<u8, M>,
    values_len: M::Offset,
    strict: bool,
) -> Result<Prepared, MemoryAccessError> {
    let (env, mut store) = ctx.data_and_store_mut();

    let function = match env
        .inner()
        .indirect_function_table_lookup(&mut store, function_id)
    {
        Ok(function) => function,
        Err(e) => return Ok(Prepared::Fail(Errno::from(e))),
    };

    let function_type = function.ty(&store);

    let memory = unsafe { env.memory_view(&store) };
    let mut current_values_offset: u64 = values.offset().into();
    let max_values_offset = current_values_offset + values_len.into();
    let mut values_buffer = vec![];
    for ty in function_type.params() {
        let Some(value) = read_value(
            &memory,
            &mut current_values_offset,
            max_values_offset,
            strict,
            ty,
        )?
        else {
            return Ok(Prepared::Fail(Errno::Inval));
        };
        values_buffer.push(value);
    }

    if strict && current_values_offset != max_values_offset {
        // If strict is true, we expect to have read all values
        return Ok(Prepared::Fail(Errno::Inval));
    }

    Ok(Prepared::Call(function, values_buffer))
}

fn finish<M: MemorySize>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    results: WasmPtr<u8, M>,
    results_len: M::Offset,
    strict: bool,
    result_values: &[Value],
) -> Result<Errno, MemoryAccessError> {
    // The environment is taken immutably here: `memory_view` needs only `&self`,
    // which leaves no laundering to do.
    let env = ctx.data();
    let store = ctx.as_store_ref();
    let memory = unsafe { env.memory_view(&store) };
    let mut current_results_offset: u64 = results.offset().into();
    let max_results_offset = current_results_offset + results_len.into();
    for result_value in result_values {
        write_value(
            &memory,
            &mut current_results_offset,
            max_results_offset,
            strict,
            result_value,
        )?;
    }

    if strict && current_results_offset != max_results_offset {
        // If strict is true, we expect to have written all results
        return Ok(Errno::Inval);
    }

    Ok(Errno::Success)
}

/// Call a function from the `__indirect_function_table` with parameters and results from memory.
///
/// This function can be used to call functions whose types are not known at
/// compile time of the caller. It is the callers responsibility to ensure
/// that the passed parameters and results match the signature of the function
/// being called.
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
) -> Result<Errno, RuntimeError> {
    let strict = matches!(strict, Bool::True);

    let (function, values_buffer) =
        match wasi_try_mem_ok!(prepare(&mut ctx, function_id, values, values_len, strict)) {
            Prepared::Call(function, values_buffer) => (function, values_buffer),
            Prepared::Fail(errno) => return Ok(errno),
        };

    // Call through `ctx` rather than a store half taken out of
    // `data_and_store_mut`, so neither half spans the call. That pair is
    // laundered past the borrow checker, and this calls an arbitrary
    // indirect-table function — every syscall it makes reborrows this same
    // environment, which invalidates the halves taken before it.
    let result_values = function
        .call(&mut ctx, values_buffer.as_slice())
        .map_err(crate::flatten_runtime_error)?;

    Ok(wasi_try_mem_ok!(finish(
        &ctx,
        results,
        results_len,
        strict,
        &result_values
    )))
}

/// [`call_dynamic`] for hosts whose guests suspend through JSPI.
///
/// The sync flavour re-enters the guest with `Function::call`, which on the JS
/// backend leaves a host frame between the guest's outer
/// `WebAssembly.promising` boundary and everything the callee runs. V8 refuses
/// to suspend past that frame ("trying to suspend JS frames"), so any
/// suspension below a dynamic call fails — and dynamic calls are how a WASIX
/// guest reaches every dlopen'd module, so on JS that rules out, for instance,
/// a native extension that switches stacks.
///
/// Awaiting `Function::call_async` instead puts a `promising` boundary *above*
/// the host frame, which makes the suspension legal. That requires this syscall
/// to be an async import — the guest's own call has to be able to suspend while
/// the callee runs — so the two flavours differ in how they are registered, not
/// just in their bodies. See `wasix_exports_32`/`wasix_exports_64`.
#[instrument(
    level = "trace",
    skip_all,
    fields(%function_id, values_ptr = values.offset().into(), results_ptr = results.offset().into()),
    ret
)]
#[allow(clippy::result_large_err)]
pub async fn call_dynamic_async<M: MemorySize>(
    ctx: AsyncFunctionEnvMut<WasiEnv>,
    function_id: u32,
    values: WasmPtr<u8, M>,
    values_len: M::Offset,
    results: WasmPtr<u8, M>,
    results_len: M::Offset,
    strict: Bool,
) -> Result<Errno, RuntimeError> {
    let strict = matches!(strict, Bool::True);

    // Each lock is released before the next await: the nested call takes the
    // store for itself, and holding one across it would deadlock.
    let prepared = {
        let mut write_lock = ctx.write().await;
        let mut sync_ctx = write_lock.as_function_env_mut();
        prepare(&mut sync_ctx, function_id, values, values_len, strict)
    };
    let (function, values_buffer) = match wasi_try_mem_ok!(prepared) {
        Prepared::Call(function, values_buffer) => (function, values_buffer),
        Prepared::Fail(errno) => return Ok(errno),
    };

    let store = ctx.as_store_async();
    let result_values = function
        .call_async(&store, values_buffer)
        .await
        .map_err(crate::flatten_runtime_error)?;

    let mut write_lock = ctx.write().await;
    let sync_ctx = write_lock.as_function_env_mut();
    Ok(wasi_try_mem_ok!(finish(
        &sync_ctx,
        results,
        results_len,
        strict,
        &result_values
    )))
}
