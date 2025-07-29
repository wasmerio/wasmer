//! Closures provide a way to generate a WASM function that wraps a generic function and an environment.
//!
//! A typical usage of this API is as follows:
//!
//! 1. Allocate a function pointer for your closure with [`closure_allocate`]
//! 2. Prepare the closure with [`closure_prepare`]
//! 3. Call function pointer
//! 4. Call [`closure_prepare`] again to redefine the function pointer
//! 5. Notify wasmer that the closure is no longer needed with [`closure_free`]

use crate::{state::DlModuleSpec, syscalls::*};
use std::{path::PathBuf, sync::atomic::AtomicUsize};
use wasm_encoder::{
    CodeSection, CustomSection, ExportKind, ExportSection, FunctionSection, GlobalType,
    ImportSection, InstructionSink, MemArg, MemoryType, RefType, TableType, TypeSection, ValType,
};
use wasmer::{imports, FunctionType, Table, Type};

use wasmer_wasix_types::wasi::WasmValueType;

// Implement helper functions for wasm_encoder::ValType
trait ValTypeOps
where
    Self: Sized,
{
    fn from_u8(value: u8) -> Result<Self, Errno>;
    fn size(&self) -> u64;
    fn store(&self, sink: &mut InstructionSink<'_>, offset: u64, memory_index: u32);
    fn load(&self, sink: &mut InstructionSink<'_>, offset: u64, memory_index: u32);
}
impl ValTypeOps for ValType {
    fn from_u8(value: u8) -> Result<Self, Errno> {
        let wasix_type = WasmValueType::try_from(value).map_err(|_| Errno::Inval)?;
        match wasix_type {
            WasmValueType::I32 => Ok(Self::I32),
            WasmValueType::I64 => Ok(Self::I64),
            WasmValueType::F32 => Ok(Self::F32),
            WasmValueType::F64 => Ok(Self::F64),
            WasmValueType::V128 => Ok(Self::V128),
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::I32 => 4,
            Self::I64 => 8,
            Self::F32 => 4,
            Self::F64 => 8,
            Self::V128 => 16,
            // Not supported in closures.
            Self::Ref(_) => panic!("Cannot get size of reference type"),
        }
    }
    fn store(&self, sink: &mut InstructionSink<'_>, offset: u64, memory_index: u32) {
        match self {
            Self::I32 => sink.i32_store(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::I64 => sink.i64_store(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::F32 => sink.f32_store(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::F64 => sink.f64_store(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::V128 => sink.v128_store(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            // Not supported in closures
            Self::Ref(_) => panic!("Cannot store reference type"),
        };
    }
    fn load(&self, sink: &mut InstructionSink<'_>, offset: u64, memory_index: u32) {
        match self {
            Self::I32 => sink.i32_load(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::I64 => sink.i64_load(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::F32 => sink.f32_load(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::F64 => sink.f64_load(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            Self::V128 => sink.v128_load(MemArg {
                offset,
                align: 0,
                memory_index,
            }),
            // Not supported in closures
            Self::Ref(_) => panic!("Cannot load reference type"),
        };
    }
}

/// Build a dynamically linkable WASM module for the given closure.
fn build_closure_wasm_bytes(
    module_name: &str,
    closure: u32,
    backing_function: u32,
    environment_offset: u64,
    argument_types: &[ValType],
    result_types: &[ValType],
) -> Vec<u8> {
    let mut wasm_module = wasm_encoder::Module::new();

    // Add dylink section
    let dylink = CustomSection {
        name: Cow::Borrowed("dylink.0"),
        data: Cow::Borrowed(&[]),
    };
    wasm_module.section(&dylink);

    // Add types section
    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);
    let on_load_function_type_index = 0;
    let mut trampoline_function_params = argument_types.to_vec();
    let mut trampoline_function_results = result_types.to_vec();
    types
        .ty()
        .function(trampoline_function_params, trampoline_function_results);
    let trampoline_function_type_index = 1;
    types
        .ty()
        .function(vec![ValType::I32, ValType::I32, ValType::I32], vec![]);
    let backing_function_type_index = 2;
    wasm_module.section(&types);

    // Add a imports section
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        MemoryType {
            minimum: 1,
            maximum: Some(65536),
            shared: true,
            memory64: false,
            page_size_log2: None,
        },
    );
    let main_memory_index = 0;
    imports.import(
        "env",
        "__indirect_function_table",
        TableType {
            element_type: RefType::FUNCREF,
            minimum: 1,
            maximum: None,
            shared: false,
            table64: false,
        },
    );
    let indirect_function_table_index = 0;
    imports.import(
        "env",
        "__stack_pointer",
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
    );
    let stack_pointer_index = 0;
    imports.import(
        "GOT.func",
        module_name,
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
    );
    let trampoline_function_pointer_index = 1;
    wasm_module.section(&imports);

    let mut functions = FunctionSection::new();
    functions.function(on_load_function_type_index);
    let on_load_function_index = 0;
    functions.function(trampoline_function_type_index);
    let trampoline_function_index = 1;
    wasm_module.section(&functions);

    // Add an export section
    // FIXME: Look into replacing this with the wasm start function
    let mut exports = ExportSection::new();
    exports.export(
        "__wasm_call_ctors",
        ExportKind::Func,
        on_load_function_index,
    );
    exports.export(module_name, ExportKind::Func, trampoline_function_index);
    wasm_module.section(&exports);

    let mut code = CodeSection::new();
    let mut on_load_function = wasm_encoder::Function::new(vec![]);
    on_load_function
        .instructions()
        .i32_const(closure as i32)
        .global_get(trampoline_function_pointer_index)
        .table_get(indirect_function_table_index)
        .table_set(indirect_function_table_index)
        .end();
    code.function(&on_load_function);

    let mut trampoline_function = wasm_encoder::Function::new(vec![(3, ValType::I32)]);
    let original_stackpointer_local: u32 = argument_types.len() as u32;
    let arguments_base_local: u32 = argument_types.len() as u32 + 1;
    let results_base_local: u32 = argument_types.len() as u32 + 2;
    let mut trampoline_function_instructions = trampoline_function.instructions();
    let values_size = argument_types
        .iter()
        .map(ValType::size)
        .sum::<u64>()
        .next_multiple_of(16);
    let results_size = result_types
        .iter()
        .map(ValType::size)
        .sum::<u64>()
        .next_multiple_of(16);
    trampoline_function_instructions
        .global_get(stack_pointer_index)
        .local_tee(original_stackpointer_local)
        .i32_const(results_size as i32)
        .i32_sub()
        .local_tee(results_base_local)
        .i32_const(values_size as i32)
        .i32_sub()
        .local_tee(arguments_base_local)
        .global_set(stack_pointer_index);
    argument_types.iter().enumerate().fold(
        (0, &mut trampoline_function_instructions),
        |mut acc, (index, ty)| {
            let size = ty.size();
            acc.1
                .local_get(arguments_base_local)
                .local_get(index as u32);
            ty.store(acc.1, acc.0, main_memory_index);
            acc.0 += size;
            acc
        },
    );
    trampoline_function_instructions
        .local_get(arguments_base_local)
        .local_get(results_base_local)
        .i32_const(environment_offset as i32)
        .i32_const(backing_function as i32)
        .call_indirect(indirect_function_table_index, backing_function_type_index);
    result_types.iter().enumerate().fold(
        (0, &mut trampoline_function_instructions),
        |mut acc, (index, ty)| {
            let size = ty.size();
            acc.1.local_get(results_base_local);
            ty.load(acc.1, acc.0, main_memory_index);
            acc.0 += size;
            acc
        },
    );
    trampoline_function_instructions
        .local_get(original_stackpointer_local)
        .global_set(stack_pointer_index)
        .end();
    code.function(&trampoline_function);
    wasm_module.section(&code);

    wasm_module.finish()
}

// Monotonically incrementing id for closures
static CLOSURE_ID: AtomicUsize = AtomicUsize::new(0);

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
#[instrument(level = "trace", fields(%backing_function, %closure), ret)]
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

    let Some(linker) = env.inner().linker().cloned() else {
        error!("Closures only work for dynamic modules.");
        return Ok(Errno::Notsup);
    };

    let argument_types = {
        let arg_offset = argument_types_ptr.offset().into();
        let arguments_slice =
            wasi_try_mem_ok!(
                WasmSlice::new(&memory, arg_offset, argument_types_length as u64)
                    .and_then(WasmSlice::access)
            );
        wasi_try_ok!(arguments_slice
            .iter()
            .map(|t: &u8| ValType::from_u8(*t))
            .collect::<Result<Vec<_>, Errno>>())
    };

    let result_types = {
        let res_offset = result_types_ptr.offset().into();
        let result_slice =
            wasi_try_mem_ok!(
                WasmSlice::new(&memory, res_offset, result_types_length as u64)
                    .and_then(WasmSlice::access)
            );
        wasi_try_ok!(result_slice
            .iter()
            .map(|t: &u8| ValType::from_u8(*t))
            .collect::<Result<Vec<_>, Errno>>())
    };

    let module_name = format!(
        "__wasix_closure_{}",
        CLOSURE_ID.fetch_add(1, Ordering::SeqCst),
    );

    let wasm_bytes = build_closure_wasm_bytes(
        &module_name,
        closure,
        backing_function,
        environment.offset().into(),
        &argument_types,
        &result_types,
    );

    let ld_library_path: [&Path; 0] = [];
    let wasm_loader = DlModuleSpec::Memory {
        module_name: &module_name,
        bytes: &wasm_bytes,
    };
    let module_handle = match linker.load_module(wasm_loader, &mut ctx) {
        Ok(m) => m,
        Err(e) => {
            // Should never happen
            panic!("Failed to load newly built in-memory module: {e}");
        }
    };

    return Ok(Errno::Success);
}
