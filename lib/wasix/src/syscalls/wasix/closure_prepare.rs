//! Closures provide a way to generate a WASM function that wraps a generic function and an environment.
//!
//! A typical usage of this API is as follows:
//!
//! 1. Allocate a function pointer for your closure with [`closure_allocate`]
//! 2. Prepare the closure with [`closure_prepare`]
//! 3. Call function pointer
//! 4. Call [`closure_prepare`] again to redefine the function pointer
//! 5. Notify wasmer that the closure is no longer needed with [`closure_free`]

use crate::{state::WasmLoader, syscalls::*};
use std::path::PathBuf;
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

/// Prepare a closure so that it can be called with a given signature.
///
/// When the closure is called after [`closure_prepare`], the arguments will be decoded and passed to the backing function together with a pointer to the environment.
///
/// The backing function needs to conform to the following signature:
///   uint8_t* values - a pointer to a buffer containing the arguments.
///   uint8_t* results - a pointer to a buffer where the results will be written.
///   void* environment - the environment that was passed to closure_prepare
///
/// `backing_function` is a pointer (index into `__indirect_function_table`) to the backing function
///
/// `closure` is a pointer (index into `__indirect_function_table`) to the closure that was obtained via [`closure_allocate`].
///
/// `argument_types_ptr` is a pointer to the argument types as a list of [`WasmValueType`]s
/// `argument_types_length` is the number of arguments
///
/// `result_types_ptr` is a pointer to the result types as a list of [`WasmValueType`]s
/// `result_types_length` is the number of results
///
/// `environment` is the closure environment that will be passed to the backing function alongside the decoded arguments and results
#[instrument(level = "trace", skip_all, ret)]
pub fn closure_prepare<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    backing_function: u32,
    closure: u32,
    argument_types_ptr: WasmPtr<u8, M>,
    argument_types_length: u32,
    result_types_ptr: WasmPtr<u8, M>,
    result_types_length: u32,
    environment: WasmPtr<u8, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };

    let mut argument_types_offset: u64 = argument_types_ptr.offset().into();
    let mut argument_types_buffer = vec![0u8; argument_types_length as usize];
    memory
        .read(argument_types_offset, &mut argument_types_buffer)
        .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
    let argument_types = match argument_types_buffer
        .into_iter()
        .map(WasmValueType::new)
        .collect::<Result<Vec<_>, Errno>>()
    {
        Ok(types) => types,
        Err(errno) => return Ok(errno),
    };

    let Some(linker) = env.inner().linker() else {
        trace!("Closures only work for dynamic modules.");
        return Ok(Errno::Notsup);
    };

    let mut result_types_offset: u64 = result_types_ptr.offset().into();
    let mut result_types_buffer = vec![0u8; result_types_length as usize];
    memory
        .read(result_types_offset, &mut result_types_buffer)
        .map_err(|e| WasiError::Exit(crate::mem_error_to_wasi(e).into()))?;
    let result_types = match result_types_buffer
        .into_iter()
        .map(WasmValueType::new)
        .collect::<Result<Vec<_>, Errno>>()
    {
        Ok(types) => types,
        Err(errno) => return Ok(errno),
    };

    let concatenated_argument_types =
        argument_types
            .iter()
            .map(|ty| ty.name())
            .fold(String::new(), |mut acc, ty| {
                acc.push_str(ty);
                acc
            });
    let concatenated_result_types =
        result_types
            .iter()
            .map(|ty| ty.name())
            .fold(String::new(), |mut acc, ty| {
                acc.push_str(ty);
                acc
            });

    let user_data_ptr = environment.offset().into();

    // TODO: Actually use random or incrementing names
    let module_name = format!(
        "__wasix_closure_{}_{}_{}_{}_{}",
        closure,
        backing_function,
        concatenated_argument_types,
        concatenated_result_types,
        user_data_ptr
    );

    let module_wat = {
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

        let store_params =
            argument_types
                .iter()
                .enumerate()
                .fold((0, String::new()), |mut acc, (index, ty)| {
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
                });

        let load_results = result_types.iter().fold((0, String::new()), |mut acc, ty| {
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
        });

        // TODO: Do this less shitty
        // TODO: No string interpolation
        format!(
            r#"
    (module
      (@dylink.0) ;; Required for dynamic libraries
      (import "env" "memory" (memory (;0;) 1 65536 shared))
      (import "env" "__indirect_function_table" (table (;0;) {backing_function} funcref))
      (import "env" "__stack_pointer" (global $__stack_pointer (;0;) (mut i32)))
      (import "GOT.func" "{module_name}" (global $trampoline_function_index (;5;) (mut i32)))
      (export "{module_name}" (func $closure_trampoline_f))
      (export "__wasix_on_load_hook" (func $__wasix_on_load_hook))
      (type $backing_function_t (func (param i32) (param i32) (param i32)))
      (func $__wasix_on_load_hook
        i32.const {closure}
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

        i32.const {backing_function}
        call_indirect (type $backing_function_t)

        {load_results}

        local.get $original_sp
        global.set $__stack_pointer
      )
    )
    "#,
            backing_function = backing_function,
            closure = closure,
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

    let ld_library_path: [&Path; 0] = [];
    let module_path = PathBuf::from(format!("/proc/closures/{}", module_name));
    let full_path = PathBuf::from(format!("/proc/closures/{}", module_name));

    let linker = linker.clone();
    let module_handle = linker.load_module(
        WasmLoader::Memory {
            module_name: &module_name,
            bytes: &wasm_bytes,
            ld_library_path: ld_library_path.as_slice(),
        },
        &mut ctx,
    );
    let module_handle = match module_handle {
        Ok(m) => m,
        Err(e) => {
            // Should never happen
            panic!("Failed to load module: {}", e);
        }
    };

    return Ok(Errno::Success);
}

/// Allocate a new slot in the __indirect_function_table for a closure
///
/// Until the slot is prepared with [`closure_prepare`], it is undefined behavior to call the function at the given index.
///
/// The slot should be freed with [`closure_free`] when it is no longer needed.
#[instrument(level = "trace", skip_all, ret)]
pub fn closure_allocate<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    closure_id: WasmPtr<u32, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let Some(linker) = env.inner().linker().cloned() else {
        trace!("Closures only work for dynamic modules.");
        return Ok(Errno::Notsup);
    };

    let function_id = match linker.allocate_closure_index(&mut ctx) {
        Ok(f) => f,
        Err(e) => {
            // Should never happen
            panic!("Failed to allocate closure index: {}", e);
        }
    };

    let (env, mut store) = ctx.data_and_store_mut();
    let memory = unsafe { env.memory_view(&store) };
    closure_id.write(&memory, function_id);
    return Ok(Errno::Success);
}

/// Free a previously allocated slot for a closure in the `__indirect_function_table`
///
/// After calling this it is undefined behavior to call the function at the given index.
#[instrument(level = "trace", skip_all, ret)]
pub fn closure_free<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    closure: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker().cloned() else {
        trace!("Closures only work for dynamic modules.");
        return Ok(Errno::Notsup);
    };

    let free_result = linker.free_closure_index(&mut ctx, closure);
    if let Err(e) = free_result {
        // Should never happen
        panic!("Failed to free closure index: {}", e);
    }

    return Ok(Errno::Success);
}
