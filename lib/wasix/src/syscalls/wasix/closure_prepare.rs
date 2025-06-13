use super::*;
use crate::syscalls::*;
use wasmer::{FunctionType, Table, Type};

// TODO: Actually use
// TODO: Move to wasix-types
// TODO: Maybe expand to cover more types
#[repr(u8)]
enum WasmValueType {
    I32 = 0,
    I64 = 1,
    F32 = 2,
    F64 = 3,
}

/// TODO: write proper documentation for this function

#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn closure_prepare<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    // The backing function that will receive the serialized parameters
    // It needs to have the following signature:
    // * pointer to the values
    // * pointer to the results
    // * pointer to the userdata
    backing_function_id: u32,
    // The ID of the function that will be registered
    function_id: u32,
    // A pointer to the types of the arguments
    argument_types_ptr: WasmPtr<u8, M>,
    // The number of arguments
    argument_types_length: u32,
    // A pointer to the types of the results
    result_types_ptr: WasmPtr<u8, M>,
    // The number of results
    result_types_length: u32,
    // Pointer to the userdata. Will be passed to the function
    user_data: WasmPtr<u8, M>,
) -> Result<(), WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let Some(stack_pointer) = ctx
        .data()
        .inner()
        .main_module_instance_handles()
        .stack_pointer
        .clone()
    else {
        return panic!("No __stack_pointer global");
    };

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };
    let mut argument_types_offset: u64 = argument_types_ptr.offset().into();
    let argument_types = (argument_types_offset
        ..(argument_types_offset + argument_types_length as u64))
        .map(|i| {
            let mut type_value = [0u8; 1];
            memory
                .read(argument_types_offset, &mut type_value)
                .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))
                .unwrap();
            argument_types_offset += 1;
            match type_value[0] {
                0 => Type::I32,
                1 => Type::I64,
                2 => Type::F32,
                3 => Type::F64,
                _ => panic!("Invalid value"),
            }
        })
        .collect::<Vec<_>>();
    let mut result_types_offset: u64 = result_types_ptr.offset().into();
    let result_types = (result_types_offset..(result_types_offset + result_types_length as u64))
        .map(|i| {
            let mut type_value = [0u8; 1];
            memory
                .read(result_types_offset, &mut type_value)
                .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))
                .unwrap();
            result_types_offset += 1;
            match type_value[0] {
                0 => Type::I32,
                1 => Type::I64,
                2 => Type::F32,
                3 => Type::F64,
                _ => panic!("Invalid value"),
            }
        })
        .collect::<Vec<_>>();

    fn wasm_type_size(ty: &Type) -> u64 {
        match ty {
            Type::I32 => 4,
            Type::I64 => 8,
            Type::F32 => 4,
            Type::F64 => 8,
            Type::V128 => 16,
            _ => 0, // Cannot be stored; should never happen
        }
    }

    // TODO: Actual memory operations
    // let memory = unsafe { env.memory() };
    // let m2 = memory.clone();
    let Some(indirect_function_table) = env.inner().main_module_indirect_function_table() else {
        // No function table is available, so we cannot call any functions dynamically.
        // TODO: This should cause a hard crash, but we return an error for now
        return panic!("No indirect_function_table");
    };

    let user_data_ptr = user_data.offset().into();

    struct ClosureData {
        argument_types: Vec<Type>,
        result_types: Vec<Type>,
        stack_pointer: Global,
        indirect_function_table: Table,
        backing_function_id: u32,
        // TODO: wasm32/64
        user_data_ptr: u64,
    }

    let new_function_env = FunctionEnv::new(
        &mut store,
        ClosureData {
            argument_types: argument_types.clone(),
            result_types: result_types.clone(),
            stack_pointer,
            indirect_function_table,
            backing_function_id,
            user_data_ptr,
        },
    );

    let cool_fn = Function::new_with_env(
        &mut store,
        &new_function_env,
        FunctionType::new(argument_types, result_types),
        |mut ctx, arguments| {
            let (env, mut store) = ctx.data_and_store_mut();
            // TODO: Care about alignment
            let values_size = env
                .argument_types
                .iter()
                .map(|ty| wasm_type_size(ty))
                .sum::<u64>();
            let results_size = env
                .result_types
                .iter()
                .map(|ty| wasm_type_size(ty))
                .sum::<u64>();

            let required_stack_size = values_size + results_size;

            // TODO: Figure out if this works with g0m0. Probably not.
            //       Potential solution: Pass a BIG allocation to register_closure and use that.
            //                           Won't work with recursive functions.
            //       Potential solution: Call malloc from the host :party:
            //       Potential solution: Detect and use g0m0
            //       Potential solution: in llvm compiler: When calling a host function while in g0m0, sync g0 before and after <- best
            //       Potential solution: Pass a allocation big enough for one set of arguments and define that the guest must copy that before calling any other function
            //                           Same for results, the guest is only allowed to write there, if the next thing it does is returning
            //                           This will have issues with threads
            let previous_stack_pointer = match env.stack_pointer.get(&mut store) {
                Value::I32(a) => a as u64,
                Value::I64(a) => a as u64,
                _ => panic!("Stack pointer is not an integer"),
            };
            let Some(new_stack_pointer) = previous_stack_pointer.checked_sub(required_stack_size)
            else {
                // TODO: Actually we need to check against the stack lower bound
                panic!("Stack overflow");
            };

            let arguments_ptr = new_stack_pointer + results_size;
            let results_ptr = new_stack_pointer;

            assert!(env.argument_types.len() == arguments.len());

            for (ty, value) in env.argument_types.iter().zip(arguments.iter()) {
                assert_eq!(*ty, value.ty());
                // TODO: Actual memory operations
            }

            env.stack_pointer
                .set(&mut store, Value::I64(new_stack_pointer as i64));

            // We need to retrieve the function from the table inside the generated function, because it could have changed
            let Some(Value::FuncRef(Some(function))) = env
                .indirect_function_table
                .get(&mut store, env.backing_function_id)
            else {
                panic!(
                    "Backing function not found in table (or not a function, or not yet prepared)"
                );
            };

            // TODO: Handle wasm64
            let Ok(function) = function.typed::<(u32, u32, u32), ()>(&store) else {
                panic!("Backing function does not have the correct signature (ptr, ptr, ptr) -> ()")
            };

            function.call(
                &mut store,
                u32::try_from(arguments_ptr).expect("Arguments pointer overflow"),
                u32::try_from(results_ptr).expect("Results pointer overflow"),
                u32::try_from(env.user_data_ptr)
                    .expect("User data pointer overflow (should never happen)"),
            );

            let results = env.result_types.iter().map(|ty| {
                match ty {
                    Type::I32 => {
                        Value::I32(0)
                    }
                    Type::I64 => {
                        Value::I64(0)
                    }
                    Type::F32 => {
                        Value::F32(0.0)
                    }
                    Type::F64 => {
                        Value::F64(0.0)
                    }
                    Type::V128 => {
                        Value::V128(0)
                    }
                    _ => panic!("Invalid result type"),
                }
            }).collect::<Vec<_>>();

            env.stack_pointer
                .set(&mut store, Value::I64(previous_stack_pointer as i64));

            Ok(results)
        },
    );

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    linker.populate_dynamic_function(&mut store, function_id, cool_fn).unwrap();

    return Ok(());
}

/// Allocate a new entry in the __indirect_function_table for a closure
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn closure_allocate<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> Result<u32, WasiRuntimeError> {
    // NOTE: The libffi API makes it trivial to pass us a allocated chunk of memory in here.
    WasiEnv::do_pending_operations(&mut ctx)?;
    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    let function_id = linker.allocate_dynamic_function(&mut store).unwrap();

    return Ok(function_id);
}

/// Free a entry in the indirect_function_table
/// 
/// The function_id must have previously been allocated by allocate_closure.
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn closure_free<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
) -> Result<(), WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    // TODO: Dont crash, when the function_id is invalid, but return a proper error.
    linker.free_dynamic_function(&mut store, function_id).unwrap();

    return Ok(());
}