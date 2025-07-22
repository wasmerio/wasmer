use super::*;
use crate::syscalls::*;
use wasmer::Type;
use wasmer_wasix_types::wasi::{ReflectionResult, WasmValueType};

fn serialize_types(types: &[Type]) -> Result<Vec<WasmValueType>, Errno> {
    types
        .iter()
        .map(|t| {
            WasmValueType::try_from(*t).map_err(|_| {
                trace!("Cannot convert type {} to WasmValueType", t);
                Errno::Inval
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Provides information about a function's signature.
///
/// ### Errors
///
/// Besides the standard error codes, `reflect_signature` may set `errno` to the
/// following values:
///
/// - `Errno::Inval`: The function pointer is not valid, i.e. it does not point to a
/// function in the indirect function table or the function has a unsupported
/// signature. The sizes in the result are undefined in this case.
/// - `Errno::Overflow`: The argument_types and result_types buffers were not big enough
/// to hold the signature. They will be left unchanged. The reflection result
/// will be valid.
#[instrument(
    level = "trace",
    skip_all,
    fields(
        %function_id,
        argument_types_ptr = argument_types.offset().into(),
        argument_types_len,
        result_types_ptr = result_types.offset().into(),
        result_types_len,
        result = result.offset().into()),
    ret
)]
pub fn reflect_signature<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    argument_types: WasmPtr<WasmValueType, M>,
    argument_types_len: u16,
    result_types: WasmPtr<WasmValueType, M>,
    result_types_len: u16,
    result: WasmPtr<ReflectionResult, M>,
) -> Result<Errno, WasiError> {
    let (env, mut store) = ctx.data_and_store_mut();

    let function_lookup_result = env
        .inner()
        .indirect_function_table_lookup(&mut store, function_id);

    let memory = unsafe { env.memory_view(&store) };
    let signature_info = result.deref(&memory);

    // Look up the function in the indirect function table
    let function = match function_lookup_result {
        Ok(f) => f,
        Err(e) => {
            trace!(
                "Failed to look up function in indirect function table: {}",
                e
            );
            wasi_try_mem_ok!(signature_info.write(ReflectionResult {
                cacheable: 0,
                arguments: 0,
                results: 0,
            }));
            return Ok(e);
        }
    };

    let Some(function) = function else {
        // Function out of bounds
        wasi_try_mem_ok!(signature_info.write(ReflectionResult {
            cacheable: 0,
            arguments: 0,
            results: 0,
        }));
        return Ok(Errno::Inval);
    };

    let is_closure = env.inner().is_closure(function_id);
    let cacheable = if is_closure { 0 } else { 1 };

    let Some(function) = function else {
        wasi_try_mem_ok!(signature_info.write(ReflectionResult {
            cacheable,
            arguments: 0,
            results: 0,
        }));
        return Ok(Errno::Inval);
    };

    let function_type = function.ty(&store);
    let arguments = function_type.params();
    let results = function_type.results();

    wasi_try_mem_ok!(signature_info.write(ReflectionResult {
        cacheable,
        arguments: arguments.len() as u16,
        results: results.len() as u16,
    }));

    if (arguments.len() as u16 > argument_types_len) {
        trace!(
            "Provided arguments buffer is too small {}/{}",
            argument_types_len,
            arguments.len()
        );
        return Ok(Errno::Overflow);
    }
    if (results.len() as u16 > result_types_len) {
        trace!(
            "Provided results buffer is too small {}/{}",
            result_types_len,
            results.len()
        );
        return Ok(Errno::Overflow);
    }

    let serialized_argument_types = wasi_try_ok!(serialize_types(function_type.params()));
    let serialized_result_types = wasi_try_ok!(serialize_types(function_type.results()));

    let mut argument_types_slice = wasi_try_mem_ok!(WasmSlice::<WasmValueType>::new(
        &memory,
        argument_types.offset().into(),
        arguments.len() as u64
    ));
    let mut result_types_slice = wasi_try_mem_ok!(WasmSlice::<WasmValueType>::new(
        &memory,
        result_types.offset().into(),
        results.len() as u64
    ));

    wasi_try_mem_ok!(argument_types_slice.write_slice(&serialized_argument_types));
    wasi_try_mem_ok!(result_types_slice.write_slice(&serialized_result_types));

    Ok(Errno::Success)
}
