use std::path::PathBuf;

use super::*;
use crate::{state::WasmLoader, syscalls::*};
use wasmer::{imports, wat2wasm, FunctionType, Table, Type};

// TODO: Move to wasix-types
#[repr(u8)]
enum WasmValueType {
    I32 = 0,
    I64 = 1,
    F32 = 2,
    F64 = 3,
}
impl WasmValueType {
    fn new(value: u8) -> Result<Self, Errno> {
        match value {
            0 => Ok(Self::I32),
            1 => Ok(Self::I64),
            2 => Ok(Self::F32),
            3 => Ok(Self::F64),
            _ => Err(Errno::Inval),
        }
    }
    fn as_u8(self) -> u8 {
        self as u8
    }
    fn size(&self) -> u64 {
        match self {
            Self::I32 => 4,
            Self::I64 => 8,
            Self::F32 => 4,
            Self::F64 => 8,
        }
    }
    fn name(&self) -> &str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
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
) -> Result<Errno, WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let module_name = format!("__closure_{}_{}", function_id, backing_function_id);

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };
    let module_wat = {
        let mut argument_types_offset: u64 = argument_types_ptr.offset().into();
        let argument_types = match (argument_types_offset
            ..(argument_types_offset + argument_types_length as u64))
            .map(|i| {
                let mut type_value = [0u8; 1];
                memory
                    .read(argument_types_offset, &mut type_value)
                    .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))
                    .unwrap();
                argument_types_offset += 1;
                WasmValueType::new(type_value[0])
            })
            .collect::<Result<Vec<_>, Errno>>() {
                Ok(types) => types,
                Err(errno) => return Ok(errno),
            };
        
        let mut result_types_offset: u64 = result_types_ptr.offset().into();
        let result_types = match (result_types_offset..(result_types_offset + result_types_length as u64))
            .map(|i| {
                let mut type_value = [0u8; 1];
                memory
                    .read(result_types_offset, &mut type_value)
                    .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))
                    .unwrap();
                result_types_offset += 1;
                WasmValueType::new(type_value[0])
            })
            .collect::<Result<Vec<_>, Errno>>() {
                Ok(types) => types,
                Err(errno) => return Ok(errno),
            };

        let user_data_ptr = user_data.offset().into();

        let values_size = argument_types
            .iter()
            .map(WasmValueType::size)
            .sum::<u64>()
            .next_multiple_of(16);
        let results_size = result_types
            .iter()
            .map(WasmValueType::size)
            .sum::<u64>()
            .next_multiple_of(16);

        let required_stack_size = values_size + results_size;

        let signature_params: String = argument_types
            .iter()
            .map(|ty| {
                let name = ty.name();
                format!("(param {})", name)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let signature_results = result_types
            .iter()
            .map(|ty| {
                let name = ty.name();
                format!("(result {})", name)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let store_params = argument_types.iter().enumerate().fold(
            (0, String::new()),
            |mut acc, (index, ty)| {
                let size = ty.size();
                let typename = ty.name();
                acc.1.push_str(
                    format!(
                        r#"
            local.get $arguments_base
            local.get {index}
            {typename}.store offset={offset}
            "#,
                        typename = typename,
                        index = index,
                        offset = acc.0
                    )
                    .as_str(),
                );
                acc.0 += size;
                acc
            },
        );

        let load_results = result_types.iter().fold(
            (0, String::new()),
            |mut acc, ty| {
                let size = ty.size();
                let typename = ty.name();
                acc.1.push_str(
                    format!(
                        r#"
            local.get $results_base
            {typename}.load offset={offset}
            "#,
                        typename = typename,
                        offset = acc.0
                    )
                    .as_str(),
                );
                acc.0 += size;
                acc
            },
        );

        // TODO: Do this less shitty
        // TODO: No string interpolation
        format!(
            r#"
    (module
      (@dylink.0) ;; Required for dynamic libraries
      (import "env" "memory" (memory (;0;) 1 65536 shared))
      (import "env" "__indirect_function_table" (table (;0;) {backing_function_id} funcref))
      (import "env" "__stack_pointer" (global $__stack_pointer (;0;) (mut i32)))
      (import "GOT.func" "{module_name}" (global $trampoline_function_index (;5;) (mut i32)))
      (export "{module_name}" (func $closure_trampoline_f))
      (export "__wasm_call_ctors" (func $link_it))
      (type $backing_function_t (func (param i32) (param i32) (param i32)))
      (type $link_it_t (func ))
      (func $link_it (type $link_it_t)
        i32.const {function_id}
        global.get $trampoline_function_index
        table.get 0
        table.set 0
      )
      (func $closure_trampoline_f {signature_params} {signature_results}
        (local $original_sp i32)
        (local $arguments_base i32)
        (local $results_base i32)
        global.get $__stack_pointer
        local.tee $original_sp
        i32.const {results_size}
        i32.sub
        local.tee $results_base
        i32.const {values_size}
        i32.sub
        local.tee $arguments_base
        global.set $__stack_pointer

        {store_params}

        local.get $arguments_base
        local.get $results_base
        i32.const {user_data_ptr}

        i32.const {backing_function_id}
        call_indirect (type $backing_function_t)

        {load_results}

        local.get $original_sp
        global.set $__stack_pointer
      )
    )
    "#,
            backing_function_id = backing_function_id,
            function_id = function_id,
            values_size = values_size,
            results_size = results_size,
            user_data_ptr = user_data_ptr,
            signature_params = signature_params,
            signature_results = signature_results,
            load_results = load_results.1,
            store_params = store_params.1,
            module_name = module_name,
        )
    };

    let wasm_bytes = wat2wasm(module_wat.as_bytes()).unwrap();

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    let ld_library_path: [&Path; 0] = [];
    let module_path = PathBuf::from(format!("/proc/closures/{}", module_name));
    let full_path = PathBuf::from(format!("/proc/closures/{}", module_name));

    let linker = linker.clone();
    let module_handle = linker.load_module(
        WasmLoader::Memory{
            module_name: &module_name,
            bytes: &wasm_bytes,
            ld_library_path: ld_library_path.as_slice(),
        },
        &mut ctx,
    ).unwrap();

    return Ok(Errno::Success);
}

/// Allocate a new entry in the __indirect_function_table for a closure
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn closure_allocate<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    closure_id: WasmPtr<u32, M>,
) -> Result<Errno, WasiRuntimeError> {
    // NOTE: The libffi API makes it trivial to pass us a allocated chunk of memory in here.
    WasiEnv::do_pending_operations(&mut ctx)?;
    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    let function_id = linker.allocate_dynamic_function(&mut store).unwrap();
    let memory = unsafe { env.memory_view(&store) };

    closure_id.write(&memory, function_id);
    return Ok(Errno::Success);
}

/// Free a entry in the indirect_function_table
///
/// The function_id must have previously been allocated by allocate_closure.
#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn closure_free<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    function_id: u32,
) -> Result<Errno, WasiRuntimeError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker() else {
        panic!("Closures only work for dynamic modules.");
    };
    // TODO: Dont crash, when the function_id is invalid, but return a proper error.
    linker
        .free_dynamic_function(&mut store, function_id)
        .unwrap();

    return Ok(Errno::Success);
}