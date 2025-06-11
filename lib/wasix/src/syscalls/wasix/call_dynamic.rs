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
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;
    
    let (env, mut store) = ctx.data_and_store_mut();

    let indirect_function_table = env.inner().main_module_indirect_function_table().unwrap();
    let function_value = indirect_function_table
        .get(&mut store, function_id)
        .unwrap();
    let function = function_value.unwrap_funcref().as_ref().unwrap();
    let function_type = function.ty(&store);
    
    let memory = unsafe { env.memory_view(&store) };
    let mut current_values_offset: u64 = values.offset().into();
    let values_buffer = function_type.params().iter().map(|ty| {
        let mut value = Value::default_typed(ty); // Initialize a default value for the type
        let buffer = value.as_slice_mut().unwrap();
        memory.read(current_values_offset, buffer);
        current_values_offset += buffer.len() as u64; // Move to the next value offset
        value
    }).collect::<Vec<_>>();

    let result_values = function.call(&mut store, values_buffer.as_slice()).unwrap();

    let memory = unsafe { env.memory_view(&store) };
    let mut current_results_offset: u64 = results.offset().into();
    result_values.iter().for_each(|result_value| {
        let Some(bytes) = result_value.as_slice() else {
            panic!("Function returned an unsupported type");
        };
        memory.write(current_results_offset, &bytes).unwrap();
        current_results_offset += bytes.len() as u64; // Move to the next result offset
    });

    Ok(Errno::Success)
}