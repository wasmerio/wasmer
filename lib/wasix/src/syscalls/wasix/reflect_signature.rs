use super::*;
use crate::syscalls::*;
use wasmer::Type;
use wasmer_wasix_types::wasi::WasmValueType;

/// A structure representing the reflection information for a function signature
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FunctionSignatureInfo {
    /// Whether the function's signature is cacheable
    pub cacheable: u8,
    /// Number of arguments the function takes
    pub arguments: u16,
    /// Number of results the function returns
    pub results: u16,
}

unsafe impl wasmer_types::ValueType for FunctionSignatureInfo {
    #[inline]
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        bytes[1].write(0);
    }
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
    result: WasmPtr<FunctionSignatureInfo, M>,
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
            wasi_try_mem_ok!(signature_info.write(FunctionSignatureInfo {
                cacheable: 0,
                arguments: 0,
                results: 0,
            }));
            return Ok(e);
        }
    };

    let Some(function) = function else {
        // Function out of bounds
        wasi_try_mem_ok!(signature_info.write(FunctionSignatureInfo {
            cacheable: 0,
            arguments: 0,
            results: 0,
        }));
        return Ok(Errno::Inval);
    };

    let is_closure = env.inner().is_closure(function_id);
    let cacheable = if is_closure { 0 } else { 1 };

    let Some(function) = function else {
        wasi_try_mem_ok!(signature_info.write(FunctionSignatureInfo {
            cacheable,
            arguments: 0,
            results: 0,
        }));
        return Ok(Errno::Inval);
    };

    let function_type = function.ty(&store);
    let arguments = function_type.params().len() as u16;
    let results = function_type.results().len() as u16;

    wasi_try_mem_ok!(signature_info.write(FunctionSignatureInfo {
        cacheable,
        arguments,
        results,
    }));

    if (arguments > argument_types_len || results > result_types_len) {
        return Ok(Errno::Overflow);
    }

    for (index, param) in function_type.params().iter().enumerate() {
        let Ok(value_type) = WasmValueType::try_from(*param) else {
            trace!(
                "Failed to convert parameter type {} to WasmValueType",
                param
            );
            return Ok(Errno::Inval);
        };
        wasi_try_mem_ok!(argument_types.write(&memory, value_type));
        wasi_try_mem_ok!(argument_types.add_offset(M::ONE));
    }

    for (index, result) in function_type.results().iter().enumerate() {
        let Ok(value_type) = WasmValueType::try_from(*result) else {
            trace!("Failed to convert result type {} to WasmValueType", result);
            return Ok(Errno::Inval);
        };
        wasi_try_mem_ok!(result_types.write(&memory, value_type));
        wasi_try_mem_ok!(result_types.add_offset(M::ONE));
    }

    Ok(Errno::Success)
}
