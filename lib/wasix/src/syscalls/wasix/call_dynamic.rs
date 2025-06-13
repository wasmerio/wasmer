use wasmer::Type;
use super::*;
use crate::syscalls::*;

/// TODO: write proper documentation for this function
/// Calls a function from the indirect function table with the given values.
/// 
/// This function can be used to call functions whose types are not known at compile time of the caller.
/// 
/// As the caller it is you responsibility
///  need to pass the correct number of values and with the correct types. 
/// 
/// function_id:
/// The ID of the function to call
/// This is an index into the indirect function table
/// 
/// values:
/// Pointer to a sequence of pointers to the values that should be passed to the function
/// If the function does not take any values, this can be a nullptr (0).
/// As the user of this function, you need to ensure that the values are in matching the actual function signature. 
/// If they dont match, its undefined behavior.
/// 
/// return_value:
/// Pointer to a location where the return value of the function will be written.
/// If the function does not return a value, this can be a nullptr (0).
/// As the user of this function, its your responsibility to ensure that the return buffer is large enough to hold the return value.
/// 
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn call_dynamic<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
    values: WasmPtr<u8, M>,
    results: WasmPtr<u8, M>,
) -> Result<Errno, WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let Some(indirect_function_table) = env.inner().main_module_indirect_function_table() else {
        // No function table is available, so we cannot call any functions dynamically.
        // TODO: This should cause a hard crash, but we return an error for now
        return Ok(Errno::Notsup);
    };
    let Some(function_value) = indirect_function_table.get(&mut store, function_id) else {
        // TODO: This should cause a trap similar to calling a function with an invalid index in the function table.
        return Ok(Errno::Inval);
    };
        
    let Value::FuncRef(Some(function)) = function_value else {
        // The table does not contain functions, but something else.
        // TODO: This should cause a crash 
        return Ok(Errno::Inval);
    };
    let function_type = function.ty(&store);
    
    let memory = unsafe { env.memory_view(&store) };
    let mut current_values_offset: u64 = values.offset().into();
    let values_buffer = function_type.params().iter().map(|ty| {
        let mut value = Value::default_typed(ty); // Initialize a default value for the type
        let buffer = value.as_slice_mut().unwrap(); // This should never fail, because a function's parameters are always valid types
        memory.read(current_values_offset, buffer).map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
        current_values_offset += buffer.len() as u64; // Move to the next value offset
        Ok(value)
    }).collect::<Result<Vec<_>, WasiRuntimeError>>()?;

    let result_values = function.call(&mut store, values_buffer.as_slice())?;

    let memory = unsafe { env.memory_view(&store) };
    let mut current_results_offset: u64 = results.offset().into();
    result_values.iter().try_for_each(|result_value| {
        let Some(bytes) = result_value.as_slice() else {
            panic!("Function returned an unsupported type");
        };
        memory.write(current_results_offset, &bytes).map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
        current_results_offset += bytes.len() as u64; // Move to the next result offset
        Result::<(), WasiRuntimeError>::Ok(())
    })?;

    Ok((Errno::Success))
}