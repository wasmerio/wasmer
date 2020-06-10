/*#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
*/
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

#[macro_use]
extern crate log;

use lazy_static::lazy_static;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::{f64, ffi::c_void};
use wasmer::{
    imports, Exportable, Function, FunctionType, Global, ImportObject, Instance, Memory,
    MemoryType, Module, NativeFunc, Pages, RuntimeError, Store, Table, TableType, Val, ValType,
};
/*use wasmer_runtime_core::{
    error::{CallError, CallResult, ResolveError},
    export::Export,
    func,
    module::ImportName,
    types::ElementType,
    vm::Ctx,
    DynFunc, IsExport,
};*/

#[cfg(unix)]
use ::libc::DIR as LibcDir;

// We use a placeholder for windows
#[cfg(not(unix))]
type LibcDir = u8;

#[macro_use]
mod macros;

// EMSCRIPTEN APIS
mod bitwise;
mod emscripten_target;
mod env;
mod errno;
mod exception;
mod exec;
mod exit;
mod inet;
mod io;
mod jmp;
mod libc;
mod linking;
mod lock;
mod math;
mod memory;
mod process;
mod pthread;
mod ptr;
mod signal;
mod storage;
mod syscalls;
mod time;
mod ucontext;
mod unistd;
mod utils;
mod varargs;

pub use self::storage::{align_memory, static_alloc};
pub use self::utils::{
    allocate_cstr_on_stack, allocate_on_stack, get_emscripten_memory_size, get_emscripten_metadata,
    get_emscripten_table_size, is_emscripten_module,
};

/// The environment provided to the Emscripten imports.
pub struct EmEnv<'a> {
    memory: Memory,
    data: &'a mut EmscriptenData<'a>,
}

impl<'a> EmEnv<'a> {
    pub fn new(memory: Memory, data: &'a mut EmscriptenData) -> Self {
        Self { memory, data }
    }

    /// Get a reference to the memory
    pub fn memory(&self, _mem_idx: u32) -> &Memory {
        &self.memory
    }
}

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5_242_880;
// TODO: make this variable
const STATIC_BUMP: u32 = 215_536;

lazy_static! {
    static ref OLD_ABORT_ON_CANNOT_GROW_MEMORY_SIG: FunctionType =
        { FunctionType::new(vec![], vec![ValType::I32]) };
}

// The address globals begin at. Very low in memory, for code size and optimization opportunities.
// Above 0 is static memory, starting with globals.
// Then the stack.
// Then 'dynamic' memory for sbrk.
const GLOBAL_BASE: u32 = 1024;
const STATIC_BASE: u32 = GLOBAL_BASE;

pub struct EmscriptenData<'a> {
    pub globals: &'a EmscriptenGlobalsData,

    pub malloc: Option<NativeFunc<'a, u32, u32>>,
    pub free: Option<NativeFunc<'a, u32>>,
    pub memalign: Option<NativeFunc<'a, (u32, u32), u32>>,
    pub memset: Option<NativeFunc<'a, (u32, u32, u32), u32>>,
    pub stack_alloc: Option<NativeFunc<'a, u32, u32>>,
    pub jumps: Vec<UnsafeCell<[u32; 27]>>,
    pub opened_dirs: HashMap<i32, Box<*mut LibcDir>>,

    pub dyn_call_i: Option<NativeFunc<'a, i32, i32>>,
    pub dyn_call_ii: Option<NativeFunc<'a, (i32, i32), i32>>,
    pub dyn_call_iii: Option<NativeFunc<'a, (i32, i32, i32), i32>>,
    pub dyn_call_iiii: Option<NativeFunc<'a, (i32, i32, i32, i32), i32>>,
    pub dyn_call_iifi: Option<NativeFunc<'a, (i32, i32, f64, i32), i32>>,
    pub dyn_call_v: Option<NativeFunc<'a, i32>>,
    pub dyn_call_vi: Option<NativeFunc<'a, (i32, i32)>>,
    pub dyn_call_vii: Option<NativeFunc<'a, (i32, i32, i32)>>,
    pub dyn_call_viii: Option<NativeFunc<'a, (i32, i32, i32, i32)>>,
    pub dyn_call_viiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32)>>,

    // round 2
    pub dyn_call_dii: Option<NativeFunc<'a, (i32, i32, i32), f64>>,
    pub dyn_call_diiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32), f64>>,
    pub dyn_call_iiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiiiiiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_vd: Option<NativeFunc<'a, (i32, f64)>>,
    pub dyn_call_viiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiiiiiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiiiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiiiiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_iij: Option<NativeFunc<'a, (i32, i32, i32, i32), i32>>,
    pub dyn_call_iji: Option<NativeFunc<'a, (i32, i32, i32, i32), i32>>,
    pub dyn_call_iiji: Option<NativeFunc<'a, (i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_iiijj: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_j: Option<NativeFunc<'a, i32, i32>>,
    pub dyn_call_ji: Option<NativeFunc<'a, (i32, i32), i32>>,
    pub dyn_call_jii: Option<NativeFunc<'a, (i32, i32, i32), i32>>,
    pub dyn_call_jij: Option<NativeFunc<'a, (i32, i32, i32, i32), i32>>,
    pub dyn_call_jjj: Option<NativeFunc<'a, (i32, i32, i32, i32, i32), i32>>,
    pub dyn_call_viiij: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiijiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiijiiiiii:
        Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viij: Option<NativeFunc<'a, (i32, i32, i32, i32, i32)>>,
    pub dyn_call_viiji: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viijiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viijj: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_vj: Option<NativeFunc<'a, (i32, i32, i32)>>,
    pub dyn_call_vjji: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_vij: Option<NativeFunc<'a, (i32, i32, i32, i32)>>,
    pub dyn_call_viji: Option<NativeFunc<'a, (i32, i32, i32, i32, i32)>>,
    pub dyn_call_vijiii: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_vijj: Option<NativeFunc<'a, (i32, i32, i32, i32, i32, i32)>>,
    pub dyn_call_viid: Option<NativeFunc<'a, (i32, i32, i32, f64)>>,
    pub dyn_call_vidd: Option<NativeFunc<'a, (i32, i32, f64, f64)>>,
    pub dyn_call_viidii: Option<NativeFunc<'a, (i32, i32, i32, f64, i32, i32)>>,
    pub dyn_call_viidddddddd:
        Option<NativeFunc<'a, (i32, i32, i32, f64, f64, f64, f64, f64, f64, f64, f64)>>,
    pub temp_ret_0: i32,

    pub stack_save: Option<NativeFunc<'a, (), i32>>,
    pub stack_restore: Option<NativeFunc<'a, i32>>,
    pub set_threw: Option<NativeFunc<'a, (i32, i32)>>,
    pub mapped_dirs: HashMap<String, PathBuf>,
}

impl<'a> EmscriptenData<'a> {
    pub fn new(
        instance: &'a mut Instance,
        globals: &'a EmscriptenGlobalsData,
        mapped_dirs: HashMap<String, PathBuf>,
    ) -> EmscriptenData<'a> {
        let malloc = instance
            .exports
            .get("_malloc")
            .or(instance.exports.get("malloc"))
            .ok();
        let free = instance
            .exports
            .get("_free")
            .or(instance.exports.get("free"))
            .ok();
        let memalign = instance
            .exports
            .get("_memalign")
            .or(instance.exports.get("memalign"))
            .ok();
        let memset = instance
            .exports
            .get("_memset")
            .or(instance.exports.get("memset"))
            .ok();
        let stack_alloc = instance.exports.get("stackAlloc").ok();

        let dyn_call_i = instance.exports.get("dynCall_i").ok();
        let dyn_call_ii = instance.exports.get("dynCall_ii").ok();
        let dyn_call_iii = instance.exports.get("dynCall_iii").ok();
        let dyn_call_iiii = instance.exports.get("dynCall_iiii").ok();
        let dyn_call_iifi = instance.exports.get("dynCall_iifi").ok();
        let dyn_call_v = instance.exports.get("dynCall_v").ok();
        let dyn_call_vi = instance.exports.get("dynCall_vi").ok();
        let dyn_call_vii = instance.exports.get("dynCall_vii").ok();
        let dyn_call_viii = instance.exports.get("dynCall_viii").ok();
        let dyn_call_viiii = instance.exports.get("dynCall_viiii").ok();

        // round 2
        let dyn_call_dii = instance.exports.get("dynCall_dii").ok();
        let dyn_call_diiii = instance.exports.get("dynCall_diiii").ok();
        let dyn_call_iiiii = instance.exports.get("dynCall_iiiii").ok();
        let dyn_call_iiiiii = instance.exports.get("dynCall_iiiiii").ok();
        let dyn_call_iiiiiii = instance.exports.get("dynCall_iiiiiii").ok();
        let dyn_call_iiiiiiii = instance.exports.get("dynCall_iiiiiiii").ok();
        let dyn_call_iiiiiiiii = instance.exports.get("dynCall_iiiiiiiii").ok();
        let dyn_call_iiiiiiiiii = instance.exports.get("dynCall_iiiiiiiiii").ok();
        let dyn_call_iiiiiiiiiii = instance.exports.get("dynCall_iiiiiiiiiii").ok();
        let dyn_call_vd = instance.exports.get("dynCall_vd").ok();
        let dyn_call_viiiii = instance.exports.get("dynCall_viiiii").ok();
        let dyn_call_viiiiii = instance.exports.get("dynCall_viiiiii").ok();
        let dyn_call_viiiiiii = instance.exports.get("dynCall_viiiiiii").ok();
        let dyn_call_viiiiiiii = instance.exports.get("dynCall_viiiiiiii").ok();
        let dyn_call_viiiiiiiii = instance.exports.get("dynCall_viiiiiiiii").ok();
        let dyn_call_viiiiiiiiii = instance.exports.get("dynCall_viiiiiiiiii").ok();
        let dyn_call_iij = instance.exports.get("dynCall_iij").ok();
        let dyn_call_iji = instance.exports.get("dynCall_iji").ok();
        let dyn_call_iiji = instance.exports.get("dynCall_iiji").ok();
        let dyn_call_iiijj = instance.exports.get("dynCall_iiijj").ok();
        let dyn_call_j = instance.exports.get("dynCall_j").ok();
        let dyn_call_ji = instance.exports.get("dynCall_ji").ok();
        let dyn_call_jii = instance.exports.get("dynCall_jii").ok();
        let dyn_call_jij = instance.exports.get("dynCall_jij").ok();
        let dyn_call_jjj = instance.exports.get("dynCall_jjj").ok();
        let dyn_call_viiij = instance.exports.get("dynCall_viiij").ok();
        let dyn_call_viiijiiii = instance.exports.get("dynCall_viiijiiii").ok();
        let dyn_call_viiijiiiiii = instance.exports.get("dynCall_viiijiiiiii").ok();
        let dyn_call_viij = instance.exports.get("dynCall_viij").ok();
        let dyn_call_viiji = instance.exports.get("dynCall_viiji").ok();
        let dyn_call_viijiii = instance.exports.get("dynCall_viijiii").ok();
        let dyn_call_viijj = instance.exports.get("dynCall_viijj").ok();
        let dyn_call_vj = instance.exports.get("dynCall_vj").ok();
        let dyn_call_vjji = instance.exports.get("dynCall_vjji").ok();
        let dyn_call_vij = instance.exports.get("dynCall_vij").ok();
        let dyn_call_viji = instance.exports.get("dynCall_viji").ok();
        let dyn_call_vijiii = instance.exports.get("dynCall_vijiii").ok();
        let dyn_call_vijj = instance.exports.get("dynCall_vijj").ok();
        let dyn_call_viid = instance.exports.get("dynCall_viid").ok();
        let dyn_call_vidd = instance.exports.get("dynCall_vidd").ok();
        let dyn_call_viidii = instance.exports.get("dynCall_viidii").ok();
        let dyn_call_viidddddddd = instance.exports.get("dynCall_viidddddddd").ok();

        let stack_save = instance.exports.get("stackSave").ok();
        let stack_restore = instance.exports.get("stackRestore").ok();
        let set_threw = instance
            .exports
            .get("_setThrew")
            .or(instance.exports.get("setThrew"))
            .ok();

        EmscriptenData {
            globals,

            malloc,
            free,
            memalign,
            memset,
            stack_alloc,
            jumps: Vec::new(),
            opened_dirs: HashMap::new(),

            dyn_call_i,
            dyn_call_ii,
            dyn_call_iii,
            dyn_call_iiii,
            dyn_call_iifi,
            dyn_call_v,
            dyn_call_vi,
            dyn_call_vii,
            dyn_call_viii,
            dyn_call_viiii,

            // round 2
            dyn_call_dii,
            dyn_call_diiii,
            dyn_call_iiiii,
            dyn_call_iiiiii,
            dyn_call_iiiiiii,
            dyn_call_iiiiiiii,
            dyn_call_iiiiiiiii,
            dyn_call_iiiiiiiiii,
            dyn_call_iiiiiiiiiii,
            dyn_call_vd,
            dyn_call_viiiii,
            dyn_call_viiiiii,
            dyn_call_viiiiiii,
            dyn_call_viiiiiiii,
            dyn_call_viiiiiiiii,
            dyn_call_viiiiiiiiii,
            dyn_call_iij,
            dyn_call_iji,
            dyn_call_iiji,
            dyn_call_iiijj,
            dyn_call_j,
            dyn_call_ji,
            dyn_call_jii,
            dyn_call_jij,
            dyn_call_jjj,
            dyn_call_viiij,
            dyn_call_viiijiiii,
            dyn_call_viiijiiiiii,
            dyn_call_viij,
            dyn_call_viiji,
            dyn_call_viijiii,
            dyn_call_viijj,
            dyn_call_vj,
            dyn_call_vjji,
            dyn_call_vij,
            dyn_call_viji,
            dyn_call_vijiii,
            dyn_call_vijj,
            dyn_call_viid,
            dyn_call_vidd,
            dyn_call_viidii,
            dyn_call_viidddddddd,
            temp_ret_0: 0,

            stack_save,
            stack_restore,
            set_threw,
            mapped_dirs,
        }
    }
}

/// Call the global constructors for C++ and set up the emscripten environment.
///
/// Note that this function does not completely set up Emscripten to be called.
/// before calling this function, please initialize `Ctx::data` with a pointer
/// to [`EmscriptenData`].
pub fn set_up_emscripten(instance: &mut Instance) -> Result<(), RuntimeError> {
    // ATINIT
    // (used by C++)
    if let Ok(func) = instance.exports.get::<Function>("globalCtors") {
        func.call(&[])?;
    }

    if let Ok(func) = instance
        .exports
        .get::<Function>("___emscripten_environ_constructor")
    {
        func.call(&[])?;
    }
    Ok(())
}

/// Call the main function in emscripten, assumes that the emscripten state is
/// set up.
///
/// If you don't want to set it up yourself, consider using [`run_emscripten_instance`].
pub fn emscripten_call_main(
    instance: &mut Instance,
    path: &str,
    args: &[&str],
) -> Result<(), RuntimeError> {
    let (func_name, main_func) = match instance.exports.get::<Function>("_main") {
        Ok(func) => Ok(("_main", func)),
        Err(_e) => match instance.exports.get::<Function>("main") {
            Ok(func) => Ok(("main", func)),
            Err(e) => Err(e),
        },
    }?;
    let num_params = main_func.signature().params().len();
    let _result = match num_params {
        2 => {
            let mut new_args = vec![path];
            new_args.extend(args);
            let (argc, argv) = store_module_arguments(instance.context_mut(), new_args);
            let func: Function = instance.exports.get(func_name)?;
            func.call(&[Val::I32(argc as i32), Val::I32(argv as i32)])?;
        }
        0 => {
            let func: Function = instance.exports.get(func_name)?;
            func.call(&[])?;
        }
        _ => {
            todo!("Update error type to be able to express this");
            /*return Err(RuntimeError:: CallError::Resolve(ResolveError::ExportWrongType {
                name: "main".to_string(),
            }))*/
        }
    };

    Ok(())
}

/// Top level function to execute emscripten
pub fn run_emscripten_instance(
    _module: &Module,
    instance: &mut Instance,
    globals: &mut EmscriptenGlobals,
    path: &str,
    args: Vec<&str>,
    entrypoint: Option<String>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> Result<(), RuntimeError> {
    let mut data = EmscriptenData::new(instance, &globals.data, mapped_dirs.into_iter().collect());
    let env = EmEnv::new(globals.memory, &mut data);

    set_up_emscripten(instance)?;

    // println!("running emscripten instance");

    if let Some(ep) = entrypoint {
        debug!("Running entry point: {}", &ep);
        let arg = unsafe { allocate_cstr_on_stack(instance.context_mut(), args[0]).0 };
        //let (argc, argv) = store_module_arguments(instance.context_mut(), args);
        let func: Function = instance.exports.get(&ep)?;
        func.call(&[Val::I32(arg as i32)])?;
    } else {
        emscripten_call_main(instance, path, &args)?;
    }

    // TODO atexit for emscripten
    // println!("{:?}", data);
    Ok(())
}

fn store_module_arguments(ctx: &mut EmEnv, args: Vec<&str>) -> (u32, u32) {
    let argc = args.len() + 1;

    let mut args_slice = vec![0; argc];
    for (slot, arg) in args_slice[0..argc].iter_mut().zip(args.iter()) {
        *slot = unsafe { allocate_cstr_on_stack(ctx, &arg).0 };
    }

    let (argv_offset, argv_slice): (_, &mut [u32]) =
        unsafe { allocate_on_stack(ctx, ((argc) * 4) as u32) };
    assert!(!argv_slice.is_empty());
    for (slot, arg) in argv_slice[0..argc].iter_mut().zip(args_slice.iter()) {
        *slot = *arg
    }
    argv_slice[argc] = 0;

    (argc as u32 - 1, argv_offset)
}

pub fn emscripten_set_up_memory(
    memory: &Memory,
    globals: &EmscriptenGlobalsData,
) -> Result<(), String> {
    let dynamictop_ptr = globals.dynamictop_ptr;
    let dynamic_base = globals.dynamic_base;

    if (dynamictop_ptr / 4) as usize >= memory.view::<u32>().len() {
        return Err("dynamictop_ptr beyond memory len".to_string());
    }
    memory.view::<u32>()[(dynamictop_ptr / 4) as usize].set(dynamic_base);
    Ok(())
}

pub struct EmscriptenGlobalsData {
    abort: u64,
    // Env namespace
    stacktop: u32,
    stack_max: u32,
    dynamictop_ptr: u32,
    dynamic_base: u32,
    memory_base: u32,
    table_base: u32,
    temp_double_ptr: u32,
    use_old_abort_on_cannot_grow_memory: bool,
}

pub struct EmscriptenGlobals {
    // The emscripten data
    pub data: EmscriptenGlobalsData,
    // The emscripten memory
    pub memory: Memory,
    pub table: Table,
    pub memory_min: Pages,
    pub memory_max: Option<Pages>,
    pub null_func_names: Vec<String>,
}

impl EmscriptenGlobals {
    pub fn new(
        store: &Store,
        module: &Module, /*, static_bump: u32 */
    ) -> Result<Self, String> {
        let mut use_old_abort_on_cannot_grow_memory = false;
        for (
            index,
            ImportName {
                namespace_index,
                name_index,
            },
        ) in &module.info().imported_functions
        {
            let namespace = module.info().namespace_table.get(*namespace_index);
            let name = module.info().name_table.get(*name_index);
            if name == "abortOnCannotGrowMemory" && namespace == "env" {
                let sig_index = module.info().func_assoc[index.convert_up(module.info())];
                let expected_sig = &module.info().signatures[sig_index];
                if *expected_sig == *OLD_ABORT_ON_CANNOT_GROW_MEMORY_SIG {
                    use_old_abort_on_cannot_grow_memory = true;
                }
                break;
            }
        }

        let (table_min, table_max) = get_emscripten_table_size(&module)?;
        let (memory_min, memory_max, shared) = get_emscripten_memory_size(&module)?;

        // Memory initialization
        let memory_type = MemoryType::new(memory_min, memory_max, shared);
        let memory = Memory::new(store, memory_type).unwrap();

        let table_type = TableType {
            element: ElementType::Anyfunc,
            minimum: table_min,
            maximum: table_max,
        };
        let table = Table::new(table_type).unwrap();

        let data = {
            let static_bump = STATIC_BUMP;

            let mut static_top = STATIC_BASE + static_bump;

            let memory_base = STATIC_BASE;
            let table_base = 0;

            let temp_double_ptr = static_top;
            static_top += 16;

            let (dynamic_base, dynamictop_ptr) =
                get_emscripten_metadata(&module)?.unwrap_or_else(|| {
                    let dynamictop_ptr = static_alloc(&mut static_top, 4);
                    (
                        align_memory(align_memory(static_top) + TOTAL_STACK),
                        dynamictop_ptr,
                    )
                });

            let stacktop = align_memory(static_top);
            let stack_max = stacktop + TOTAL_STACK;

            EmscriptenGlobalsData {
                abort: 0,
                stacktop,
                stack_max,
                dynamictop_ptr,
                dynamic_base,
                memory_base,
                table_base,
                temp_double_ptr,
                use_old_abort_on_cannot_grow_memory,
            }
        };

        emscripten_set_up_memory(&memory, &data)?;

        let mut null_func_names = vec![];
        for (
            _,
            ImportName {
                namespace_index,
                name_index,
            },
        ) in &module.info().imported_functions
        {
            let namespace = module.info().namespace_table.get(*namespace_index);
            let name = module.info().name_table.get(*name_index);
            if namespace == "env" && name.starts_with("nullFunction_") {
                null_func_names.push(name.to_string())
            }
        }

        Ok(Self {
            data,
            memory,
            table,
            memory_min,
            memory_max,
            null_func_names,
        })
    }
}

pub fn generate_emscripten_env(
    globals: &mut EmscriptenGlobals,
    store: &Store,
    env: &mut EmEnv,
) -> ImportObject {
    let abort_on_cannot_grow_memory_export = if globals.data.use_old_abort_on_cannot_grow_memory {
        Function::new_env(store, env, crate::memory::abort_on_cannot_grow_memory_old).to_export()
    } else {
        Function::new_env(store, env, crate::memory::abort_on_cannot_grow_memory).to_export()
    };

    let mut env_ns = namespace! {
        "memory" => Export::Memory(globals.memory.clone()),
        "table" => Export::Table(globals.table.clone()),

        // Globals
        "STACKTOP" => Global::new(Value::I32(globals.data.stacktop as i32)),
        "STACK_MAX" => Global::new(Value::I32(globals.data.stack_max as i32)),
        "DYNAMICTOP_PTR" => Global::new(Value::I32(globals.data.dynamictop_ptr as i32)),
        "fb" => Global::new(Value::I32(globals.data.table_base as i32)),
        "tableBase" => Global::new(Value::I32(globals.data.table_base as i32)),
        "__table_base" => Global::new(Value::I32(globals.data.table_base as i32)),
        "ABORT" => Global::new(Value::I32(globals.data.abort as i32)),
        "gb" => Global::new(Value::I32(globals.data.memory_base as i32)),
        "memoryBase" => Global::new(Value::I32(globals.data.memory_base as i32)),
        "__memory_base" => Global::new(Value::I32(globals.data.memory_base as i32)),
        "tempDoublePtr" => Global::new(Value::I32(globals.data.temp_double_ptr as i32)),

        // inet
        "_inet_addr" => Function::new_env(store, env, crate::inet::addr),

        // IO
        "printf" => Function::new_env(store, env, crate::io::printf),
        "putchar" => Function::new_env(store, env, crate::io::putchar),
        "___lock" => Function::new_env(store, env, crate::lock::___lock),
        "___unlock" => Function::new_env(store, env, crate::lock::___unlock),
        "___wait" => Function::new_env(store, env, crate::lock::___wait),
        "_flock" => Function::new_env(store, env, crate::lock::_flock),
        "_chroot" => Function::new_env(store, env, crate::io::chroot),
        "_getprotobyname" => Function::new_env(store, env, crate::io::getprotobyname),
        "_getprotobynumber" => Function::new_env(store, env, crate::io::getprotobynumber),
        "_getpwuid" => Function::new_env(store, env, crate::io::getpwuid),
        "_sigdelset" => Function::new_env(store, env, crate::io::sigdelset),
        "_sigfillset" => Function::new_env(store, env, crate::io::sigfillset),
        "_tzset" => Function::new_env(store, env, crate::io::tzset),
        "_strptime" => Function::new_env(store, env, crate::io::strptime),

        // exec
        "_execvp" => Function::new_env(store, env, crate::exec::execvp),
        "_execl" => Function::new_env(store, env, crate::exec::execl),
        "_execle" => Function::new_env(store, env, crate::exec::execle),

        // exit
        "__exit" => Function::new_env(store, env, crate::exit::exit),

        // Env
        "___assert_fail" => Function::new_env(store, env, crate::env::___assert_fail),
        "_getenv" => Function::new_env(store, env, crate::env::_getenv),
        "_setenv" => Function::new_env(store, env, crate::env::_setenv),
        "_putenv" => Function::new_env(store, env, crate::env::_putenv),
        "_unsetenv" => Function::new_env(store, env, crate::env::_unsetenv),
        "_getpwnam" => Function::new_env(store, env, crate::env::_getpwnam),
        "_getgrnam" => Function::new_env(store, env, crate::env::_getgrnam),
        "___buildEnvironment" => Function::new_env(store, env, crate::env::___build_environment),
        "___setErrNo" => Function::new_env(store, env, crate::errno::___seterrno),
        "_getpagesize" => Function::new_env(store, env, crate::env::_getpagesize),
        "_sysconf" => Function::new_env(store, env, crate::env::_sysconf),
        "_getaddrinfo" => Function::new_env(store, env, crate::env::_getaddrinfo),
        "_times" => Function::new_env(store, env, crate::env::_times),
        "_pathconf" => Function::new_env(store, env, crate::env::_pathconf),
        "_fpathconf" => Function::new_env(store, env, crate::env::_fpathconf),

        // Syscalls
        "___syscall1" => Function::new_env(store, env, crate::syscalls::___syscall1),
        "___syscall3" => Function::new_env(store, env, crate::syscalls::___syscall3),
        "___syscall4" => Function::new_env(store, env, crate::syscalls::___syscall4),
        "___syscall5" => Function::new_env(store, env, crate::syscalls::___syscall5),
        "___syscall6" => Function::new_env(store, env, crate::syscalls::___syscall6),
        "___syscall9" => Function::new_env(store, env, crate::syscalls::___syscall9),
        "___syscall10" => Function::new_env(store, env, crate::syscalls::___syscall10),
        "___syscall12" => Function::new_env(store, env, crate::syscalls::___syscall12),
        "___syscall14" => Function::new_env(store, env, crate::syscalls::___syscall14),
        "___syscall15" => Function::new_env(store, env, crate::syscalls::___syscall15),
        "___syscall20" => Function::new_env(store, env, crate::syscalls::___syscall20),
        "___syscall21" => Function::new_env(store, env, crate::syscalls::___syscall21),
        "___syscall25" => Function::new_env(store, env, crate::syscalls::___syscall25),
        "___syscall29" => Function::new_env(store, env, crate::syscalls::___syscall29),
        "___syscall32" => Function::new_env(store, env, crate::syscalls::___syscall32),
        "___syscall33" => Function::new_env(store, env, crate::syscalls::___syscall33),
        "___syscall34" => Function::new_env(store, env, crate::syscalls::___syscall34),
        "___syscall36" => Function::new_env(store, env, crate::syscalls::___syscall36),
        "___syscall39" => Function::new_env(store, env, crate::syscalls::___syscall39),
        "___syscall38" => Function::new_env(store, env, crate::syscalls::___syscall38),
        "___syscall40" => Function::new_env(store, env, crate::syscalls::___syscall40),
        "___syscall41" => Function::new_env(store, env, crate::syscalls::___syscall41),
        "___syscall42" => Function::new_env(store, env, crate::syscalls::___syscall42),
        "___syscall51" => Function::new_env(store, env, crate::syscalls::___syscall51),
        "___syscall52" => Function::new_env(store, env, crate::syscalls::___syscall52),
        "___syscall53" => Function::new_env(store, env, crate::syscalls::___syscall53),
        "___syscall54" => Function::new_env(store, env, crate::syscalls::___syscall54),
        "___syscall57" => Function::new_env(store, env, crate::syscalls::___syscall57),
        "___syscall60" => Function::new_env(store, env, crate::syscalls::___syscall60),
        "___syscall63" => Function::new_env(store, env, crate::syscalls::___syscall63),
        "___syscall64" => Function::new_env(store, env, crate::syscalls::___syscall64),
        "___syscall66" => Function::new_env(store, env, crate::syscalls::___syscall66),
        "___syscall75" => Function::new_env(store, env, crate::syscalls::___syscall75),
        "___syscall77" => Function::new_env(store, env, crate::syscalls::___syscall77),
        "___syscall83" => Function::new_env(store, env, crate::syscalls::___syscall83),
        "___syscall85" => Function::new_env(store, env, crate::syscalls::___syscall85),
        "___syscall91" => Function::new_env(store, env, crate::syscalls::___syscall91),
        "___syscall94" => Function::new_env(store, env, crate::syscalls::___syscall94),
        "___syscall96" => Function::new_env(store, env, crate::syscalls::___syscall96),
        "___syscall97" => Function::new_env(store, env, crate::syscalls::___syscall97),
        "___syscall102" => Function::new_env(store, env, crate::syscalls::___syscall102),
        "___syscall110" => Function::new_env(store, env, crate::syscalls::___syscall110),
        "___syscall114" => Function::new_env(store, env, crate::syscalls::___syscall114),
        "___syscall118" => Function::new_env(store, env, crate::syscalls::___syscall118),
        "___syscall121" => Function::new_env(store, env, crate::syscalls::___syscall121),
        "___syscall122" => Function::new_env(store, env, crate::syscalls::___syscall122),
        "___syscall125" => Function::new_env(store, env, crate::syscalls::___syscall125),
        "___syscall132" => Function::new_env(store, env, crate::syscalls::___syscall132),
        "___syscall133" => Function::new_env(store, env, crate::syscalls::___syscall133),
        "___syscall140" => Function::new_env(store, env, crate::syscalls::___syscall140),
        "___syscall142" => Function::new_env(store, env, crate::syscalls::___syscall142),
        "___syscall144" => Function::new_env(store, env, crate::syscalls::___syscall144),
        "___syscall145" => Function::new_env(store, env, crate::syscalls::___syscall145),
        "___syscall146" => Function::new_env(store, env, crate::syscalls::___syscall146),
        "___syscall147" => Function::new_env(store, env, crate::syscalls::___syscall147),
        "___syscall148" => Function::new_env(store, env, crate::syscalls::___syscall148),
        "___syscall150" => Function::new_env(store, env, crate::syscalls::___syscall150),
        "___syscall151" => Function::new_env(store, env, crate::syscalls::___syscall151),
        "___syscall152" => Function::new_env(store, env, crate::syscalls::___syscall152),
        "___syscall153" => Function::new_env(store, env, crate::syscalls::___syscall153),
        "___syscall163" => Function::new_env(store, env, crate::syscalls::___syscall163),
        "___syscall168" => Function::new_env(store, env, crate::syscalls::___syscall168),
        "___syscall180" => Function::new_env(store, env, crate::syscalls::___syscall180),
        "___syscall181" => Function::new_env(store, env, crate::syscalls::___syscall181),
        "___syscall183" => Function::new_env(store, env, crate::syscalls::___syscall183),
        "___syscall191" => Function::new_env(store, env, crate::syscalls::___syscall191),
        "___syscall192" => Function::new_env(store, env, crate::syscalls::___syscall192),
        "___syscall193" => Function::new_env(store, env, crate::syscalls::___syscall193),
        "___syscall194" => Function::new_env(store, env, crate::syscalls::___syscall194),
        "___syscall195" => Function::new_env(store, env, crate::syscalls::___syscall195),
        "___syscall196" => Function::new_env(store, env, crate::syscalls::___syscall196),
        "___syscall197" => Function::new_env(store, env, crate::syscalls::___syscall197),
        "___syscall198" => Function::new_env(store, env, crate::syscalls::___syscall198),
        "___syscall199" => Function::new_env(store, env, crate::syscalls::___syscall199),
        "___syscall200" => Function::new_env(store, env, crate::syscalls::___syscall200),
        "___syscall201" => Function::new_env(store, env, crate::syscalls::___syscall201),
        "___syscall202" => Function::new_env(store, env, crate::syscalls::___syscall202),
        "___syscall205" => Function::new_env(store, env, crate::syscalls::___syscall205),
        "___syscall207" => Function::new_env(store, env, crate::syscalls::___syscall207),
        "___syscall209" => Function::new_env(store, env, crate::syscalls::___syscall209),
        "___syscall211" => Function::new_env(store, env, crate::syscalls::___syscall211),
        "___syscall212" => Function::new_env(store, env, crate::syscalls::___syscall212),
        "___syscall218" => Function::new_env(store, env, crate::syscalls::___syscall218),
        "___syscall219" => Function::new_env(store, env, crate::syscalls::___syscall219),
        "___syscall220" => Function::new_env(store, env, crate::syscalls::___syscall220),
        "___syscall221" => Function::new_env(store, env, crate::syscalls::___syscall221),
        "___syscall268" => Function::new_env(store, env, crate::syscalls::___syscall268),
        "___syscall269" => Function::new_env(store, env, crate::syscalls::___syscall269),
        "___syscall272" => Function::new_env(store, env, crate::syscalls::___syscall272),
        "___syscall295" => Function::new_env(store, env, crate::syscalls::___syscall295),
        "___syscall296" => Function::new_env(store, env, crate::syscalls::___syscall296),
        "___syscall297" => Function::new_env(store, env, crate::syscalls::___syscall297),
        "___syscall298" => Function::new_env(store, env, crate::syscalls::___syscall298),
        "___syscall300" => Function::new_env(store, env, crate::syscalls::___syscall300),
        "___syscall301" => Function::new_env(store, env, crate::syscalls::___syscall301),
        "___syscall302" => Function::new_env(store, env, crate::syscalls::___syscall302),
        "___syscall303" => Function::new_env(store, env, crate::syscalls::___syscall303),
        "___syscall304" => Function::new_env(store, env, crate::syscalls::___syscall304),
        "___syscall305" => Function::new_env(store, env, crate::syscalls::___syscall305),
        "___syscall306" => Function::new_env(store, env, crate::syscalls::___syscall306),
        "___syscall307" => Function::new_env(store, env, crate::syscalls::___syscall307),
        "___syscall308" => Function::new_env(store, env, crate::syscalls::___syscall308),
        "___syscall320" => Function::new_env(store, env, crate::syscalls::___syscall320),
        "___syscall324" => Function::new_env(store, env, crate::syscalls::___syscall324),
        "___syscall330" => Function::new_env(store, env, crate::syscalls::___syscall330),
        "___syscall331" => Function::new_env(store, env, crate::syscalls::___syscall331),
        "___syscall333" => Function::new_env(store, env, crate::syscalls::___syscall333),
        "___syscall334" => Function::new_env(store, env, crate::syscalls::___syscall334),
        "___syscall337" => Function::new_env(store, env, crate::syscalls::___syscall337),
        "___syscall340" => Function::new_env(store, env, crate::syscalls::___syscall340),
        "___syscall345" => Function::new_env(store, env, crate::syscalls::___syscall345),

        // Process
        "abort" => Function::new_env(store, env, crate::process::em_abort),
        "_abort" => Function::new_env(store, env, crate::process::_abort),
        "_prctl" => Function::new_env(store, env, crate::process::_prctl),
        "abortStackOverflow" => Function::new_env(store, env, crate::process::abort_stack_overflow),
        "_llvm_trap" => Function::new_env(store, env, crate::process::_llvm_trap),
        "_fork" => Function::new_env(store, env, crate::process::_fork),
        "_exit" => Function::new_env(store, env, crate::process::_exit),
        "_system" => Function::new_env(store, env, crate::process::_system),
        "_popen" => Function::new_env(store, env, crate::process::_popen),
        "_endgrent" => Function::new_env(store, env, crate::process::_endgrent),
        "_execve" => Function::new_env(store, env, crate::process::_execve),
        "_kill" => Function::new_env(store, env, crate::process::_kill),
        "_llvm_stackrestore" => Function::new_env(store, env, crate::process::_llvm_stackrestore),
        "_llvm_stacksave" => Function::new_env(store, env, crate::process::_llvm_stacksave),
        "_llvm_eh_typeid_for" => Function::new_env(store, env, crate::process::_llvm_eh_typeid_for),
        "_raise" => Function::new_env(store, env, crate::process::_raise),
        "_sem_init" => Function::new_env(store, env, crate::process::_sem_init),
        "_sem_destroy" => Function::new_env(store, env, crate::process::_sem_destroy),
        "_sem_post" => Function::new_env(store, env, crate::process::_sem_post),
        "_sem_wait" => Function::new_env(store, env, crate::process::_sem_wait),
        "_getgrent" => Function::new_env(store, env, crate::process::_getgrent),
        "_sched_yield" => Function::new_env(store, env, crate::process::_sched_yield),
        "_setgrent" => Function::new_env(store, env, crate::process::_setgrent),
        "_setgroups" => Function::new_env(store, env, crate::process::_setgroups),
        "_setitimer" => Function::new_env(store, env, crate::process::_setitimer),
        "_usleep" => Function::new_env(store, env, crate::process::_usleep),
        "_nanosleep" => Function::new_env(store, env, crate::process::_nanosleep),
        "_utime" => Function::new_env(store, env, crate::process::_utime),
        "_utimes" => Function::new_env(store, env, crate::process::_utimes),
        "_wait" => Function::new_env(store, env, crate::process::_wait),
        "_wait3" => Function::new_env(store, env, crate::process::_wait3),
        "_wait4" => Function::new_env(store, env, crate::process::_wait4),
        "_waitid" => Function::new_env(store, env, crate::process::_waitid),
        "_waitpid" => Function::new_env(store, env, crate::process::_waitpid),

        // Emscripten
        "_emscripten_asm_const_i" => Function::new_env(store, env, crate::emscripten_target::asm_const_i),
        "_emscripten_exit_with_live_runtime" => Function::new_env(store, env, crate::emscripten_target::exit_with_live_runtime),

        // Signal
        "_sigemptyset" => Function::new_env(store, env, crate::signal::_sigemptyset),
        "_sigaddset" => Function::new_env(store, env, crate::signal::_sigaddset),
        "_sigprocmask" => Function::new_env(store, env, crate::signal::_sigprocmask),
        "_sigaction" => Function::new_env(store, env, crate::signal::_sigaction),
        "_signal" => Function::new_env(store, env, crate::signal::_signal),
        "_sigsuspend" => Function::new_env(store, env, crate::signal::_sigsuspend),

        // Memory
        "abortOnCannotGrowMemory" => abort_on_cannot_grow_memory_export,
        "_emscripten_memcpy_big" => Function::new_env(store, env, crate::memory::_emscripten_memcpy_big),
        "_emscripten_get_heap_size" => Function::new_env(store, env, crate::memory::_emscripten_get_heap_size),
        "_emscripten_resize_heap" => Function::new_env(store, env, crate::memory::_emscripten_resize_heap),
        "enlargeMemory" => Function::new_env(store, env, crate::memory::enlarge_memory),
        "segfault" => Function::new_env(store, env, crate::memory::segfault),
        "alignfault" => Function::new_env(store, env, crate::memory::alignfault),
        "ftfault" => Function::new_env(store, env, crate::memory::ftfault),
        "getTotalMemory" => Function::new_env(store, env, crate::memory::get_total_memory),
        "_sbrk" => Function::new_env(store, env, crate::memory::sbrk),
        "___map_file" => Function::new_env(store, env, crate::memory::___map_file),

        // Exception
        "___cxa_allocate_exception" => Function::new_env(store, env, crate::exception::___cxa_allocate_exception),
        "___cxa_current_primary_exception" => Function::new_env(store, env, crate::exception::___cxa_current_primary_exception),
        "___cxa_decrement_exception_refcount" => Function::new_env(store, env, crate::exception::___cxa_decrement_exception_refcount),
        "___cxa_increment_exception_refcount" => Function::new_env(store, env, crate::exception::___cxa_increment_exception_refcount),
        "___cxa_rethrow_primary_exception" => Function::new_env(store, env, crate::exception::___cxa_rethrow_primary_exception),
        "___cxa_throw" => Function::new_env(store, env, crate::exception::___cxa_throw),
        "___cxa_begin_catch" => Function::new_env(store, env, crate::exception::___cxa_begin_catch),
        "___cxa_end_catch" => Function::new_env(store, env, crate::exception::___cxa_end_catch),
        "___cxa_uncaught_exception" => Function::new_env(store, env, crate::exception::___cxa_uncaught_exception),
        "___cxa_pure_virtual" => Function::new_env(store, env, crate::exception::___cxa_pure_virtual),

        // Time
        "_gettimeofday" => Function::new_env(store, env, crate::time::_gettimeofday),
        "_clock_getres" => Function::new_env(store, env, crate::time::_clock_getres),
        "_clock_gettime" => Function::new_env(store, env, crate::time::_clock_gettime),
        "_clock_settime" => Function::new_env(store, env, crate::time::_clock_settime),
        "___clock_gettime" => Function::new_env(store, env, crate::time::_clock_gettime),
        "_clock" => Function::new_env(store, env, crate::time::_clock),
        "_difftime" => Function::new_env(store, env, crate::time::_difftime),
        "_asctime" => Function::new_env(store, env, crate::time::_asctime),
        "_asctime_r" => Function::new_env(store, env, crate::time::_asctime_r),
        "_localtime" => Function::new_env(store, env, crate::time::_localtime),
        "_time" => Function::new_env(store, env, crate::time::_time),
        "_timegm" => Function::new_env(store, env, crate::time::_timegm),
        "_strftime" => Function::new_env(store, env, crate::time::_strftime),
        "_strftime_l" => Function::new_env(store, env, crate::time::_strftime_l),
        "_localtime_r" => Function::new_env(store, env, crate::time::_localtime_r),
        "_gmtime_r" => Function::new_env(store, env, crate::time::_gmtime_r),
        "_ctime" => Function::new_env(store, env, crate::time::_ctime),
        "_ctime_r" => Function::new_env(store, env, crate::time::_ctime_r),
        "_mktime" => Function::new_env(store, env, crate::time::_mktime),
        "_gmtime" => Function::new_env(store, env, crate::time::_gmtime),

        // Math
        "sqrt" => Function::new_env(store, env, crate::math::sqrt),
        "floor" => Function::new_env(store, env, crate::math::floor),
        "fabs" => Function::new_env(store, env, crate::math::fabs),
        "f64-rem" => Function::new_env(store, env, crate::math::f64_rem),
        "_llvm_copysign_f32" => Function::new_env(store, env, crate::math::_llvm_copysign_f32),
        "_llvm_copysign_f64" => Function::new_env(store, env, crate::math::_llvm_copysign_f64),
        "_llvm_log10_f64" => Function::new_env(store, env, crate::math::_llvm_log10_f64),
        "_llvm_log2_f64" => Function::new_env(store, env, crate::math::_llvm_log2_f64),
        "_llvm_log10_f32" => Function::new_env(store, env, crate::math::_llvm_log10_f32),
        "_llvm_log2_f32" => Function::new_env(store, env, crate::math::_llvm_log2_f64),
        "_llvm_sin_f64" => Function::new_env(store, env, crate::math::_llvm_sin_f64),
        "_llvm_cos_f64" => Function::new_env(store, env, crate::math::_llvm_cos_f64),
        "_llvm_exp2_f32" => Function::new_env(store, env, crate::math::_llvm_exp2_f32),
        "_llvm_exp2_f64" => Function::new_env(store, env, crate::math::_llvm_exp2_f64),
        "_llvm_trunc_f64" => Function::new_env(store, env, crate::math::_llvm_trunc_f64),
        "_llvm_fma_f64" => Function::new_env(store, env, crate::math::_llvm_fma_f64),
        "_emscripten_random" => Function::new_env(store, env, crate::math::_emscripten_random),

        // Jump
        "__setjmp" => Function::new_env(store, env, crate::jmp::__setjmp),
        "__longjmp" => Function::new_env(store, env, crate::jmp::__longjmp),
        "_longjmp" => Function::new_env(store, env, crate::jmp::_longjmp),
        "_emscripten_longjmp" => Function::new_env(store, env, crate::jmp::_longjmp),

        // Bitwise
        "_llvm_bswap_i64" => Function::new_env(store, env, crate::bitwise::_llvm_bswap_i64),

        // libc
        "_execv" => Function::new_env(store, env, crate::libc::execv),
        "_endpwent" => Function::new_env(store, env, crate::libc::endpwent),
        "_fexecve" => Function::new_env(store, env, crate::libc::fexecve),
        "_fpathconf" => Function::new_env(store, env, crate::libc::fpathconf),
        "_getitimer" => Function::new_env(store, env, crate::libc::getitimer),
        "_getpwent" => Function::new_env(store, env, crate::libc::getpwent),
        "_killpg" => Function::new_env(store, env, crate::libc::killpg),
        "_pathconf" => Function::new_env(store, env, crate::libc::pathconf),
        "_siginterrupt" => Function::new_env(store, env, crate::signal::_siginterrupt),
        "_setpwent" => Function::new_env(store, env, crate::libc::setpwent),
        "_sigismember" => Function::new_env(store, env, crate::libc::sigismember),
        "_sigpending" => Function::new_env(store, env, crate::libc::sigpending),
        "___libc_current_sigrtmax" => Function::new_env(store, env, crate::libc::current_sigrtmax),
        "___libc_current_sigrtmin" => Function::new_env(store, env, crate::libc::current_sigrtmin),

        // Linking
        "_dlclose" => Function::new_env(store, env, crate::linking::_dlclose),
        "_dlerror" => Function::new_env(store, env, crate::linking::_dlerror),
        "_dlopen" => Function::new_env(store, env, crate::linking::_dlopen),
        "_dlsym" => Function::new_env(store, env, crate::linking::_dlsym),

        // wasm32-unknown-emscripten
        "_alarm" => Function::new_env(store, env, crate::emscripten_target::_alarm),
        "_atexit" => Function::new_env(store, env, crate::emscripten_target::_atexit),
        "setTempRet0" => Function::new_env(store, env, crate::emscripten_target::setTempRet0),
        "getTempRet0" => Function::new_env(store, env, crate::emscripten_target::getTempRet0),
        "invoke_i" => Function::new_env(store, env, crate::emscripten_target::invoke_i),
        "invoke_ii" => Function::new_env(store, env, crate::emscripten_target::invoke_ii),
        "invoke_iii" => Function::new_env(store, env, crate::emscripten_target::invoke_iii),
        "invoke_iiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiii),
        "invoke_iifi" => Function::new_env(store, env, crate::emscripten_target::invoke_iifi),
        "invoke_v" => Function::new_env(store, env, crate::emscripten_target::invoke_v),
        "invoke_vi" => Function::new_env(store, env, crate::emscripten_target::invoke_vi),
        "invoke_vj" => Function::new_env(store, env, crate::emscripten_target::invoke_vj),
        "invoke_vjji" => Function::new_env(store, env, crate::emscripten_target::invoke_vjji),
        "invoke_vii" => Function::new_env(store, env, crate::emscripten_target::invoke_vii),
        "invoke_viii" => Function::new_env(store, env, crate::emscripten_target::invoke_viii),
        "invoke_viiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiii),
        "__Unwind_Backtrace" => Function::new_env(store, env, crate::emscripten_target::__Unwind_Backtrace),
        "__Unwind_FindEnclosingFunction" => Function::new_env(store, env, crate::emscripten_target::__Unwind_FindEnclosingFunction),
        "__Unwind_GetIPInfo" => Function::new_env(store, env, crate::emscripten_target::__Unwind_GetIPInfo),
        "___cxa_find_matching_catch_2" => Function::new_env(store, env, crate::emscripten_target::___cxa_find_matching_catch_2),
        "___cxa_find_matching_catch_3" => Function::new_env(store, env, crate::emscripten_target::___cxa_find_matching_catch_3),
        "___cxa_free_exception" => Function::new_env(store, env, crate::emscripten_target::___cxa_free_exception),
        "___resumeException" => Function::new_env(store, env, crate::emscripten_target::___resumeException),
        "_dladdr" => Function::new_env(store, env, crate::emscripten_target::_dladdr),
        "_pthread_attr_destroy" => Function::new_env(store, env, crate::pthread::_pthread_attr_destroy),
        "_pthread_attr_getstack" => Function::new_env(store, env, crate::pthread::_pthread_attr_getstack),
        "_pthread_attr_init" => Function::new_env(store, env, crate::pthread::_pthread_attr_init),
        "_pthread_attr_setstacksize" => Function::new_env(store, env, crate::pthread::_pthread_attr_setstacksize),
        "_pthread_cleanup_pop" => Function::new_env(store, env, crate::pthread::_pthread_cleanup_pop),
        "_pthread_cleanup_push" => Function::new_env(store, env, crate::pthread::_pthread_cleanup_push),
        "_pthread_cond_destroy" => Function::new_env(store, env, crate::pthread::_pthread_cond_destroy),
        "_pthread_cond_init" => Function::new_env(store, env, crate::pthread::_pthread_cond_init),
        "_pthread_cond_signal" => Function::new_env(store, env, crate::pthread::_pthread_cond_signal),
        "_pthread_cond_timedwait" => Function::new_env(store, env, crate::pthread::_pthread_cond_timedwait),
        "_pthread_cond_wait" => Function::new_env(store, env, crate::pthread::_pthread_cond_wait),
        "_pthread_condattr_destroy" => Function::new_env(store, env, crate::pthread::_pthread_condattr_destroy),
        "_pthread_condattr_init" => Function::new_env(store, env, crate::pthread::_pthread_condattr_init),
        "_pthread_condattr_setclock" => Function::new_env(store, env, crate::pthread::_pthread_condattr_setclock),
        "_pthread_create" => Function::new_env(store, env, crate::pthread::_pthread_create),
        "_pthread_detach" => Function::new_env(store, env, crate::pthread::_pthread_detach),
        "_pthread_equal" => Function::new_env(store, env, crate::pthread::_pthread_equal),
        "_pthread_exit" => Function::new_env(store, env, crate::pthread::_pthread_exit),
        "_pthread_self" => Function::new_env(store, env, crate::pthread::_pthread_self),
        "_pthread_getattr_np" => Function::new_env(store, env, crate::pthread::_pthread_getattr_np),
        "_pthread_getspecific" => Function::new_env(store, env, crate::pthread::_pthread_getspecific),
        "_pthread_join" => Function::new_env(store, env, crate::pthread::_pthread_join),
        "_pthread_key_create" => Function::new_env(store, env, crate::pthread::_pthread_key_create),
        "_pthread_mutex_destroy" => Function::new_env(store, env, crate::pthread::_pthread_mutex_destroy),
        "_pthread_mutex_init" => Function::new_env(store, env, crate::pthread::_pthread_mutex_init),
        "_pthread_mutexattr_destroy" => Function::new_env(store, env, crate::pthread::_pthread_mutexattr_destroy),
        "_pthread_mutexattr_init" => Function::new_env(store, env, crate::pthread::_pthread_mutexattr_init),
        "_pthread_mutexattr_settype" => Function::new_env(store, env, crate::pthread::_pthread_mutexattr_settype),
        "_pthread_once" => Function::new_env(store, env, crate::pthread::_pthread_once),
        "_pthread_rwlock_destroy" => Function::new_env(store, env, crate::pthread::_pthread_rwlock_destroy),
        "_pthread_rwlock_init" => Function::new_env(store, env, crate::pthread::_pthread_rwlock_init),
        "_pthread_rwlock_rdlock" => Function::new_env(store, env, crate::pthread::_pthread_rwlock_rdlock),
        "_pthread_rwlock_unlock" => Function::new_env(store, env, crate::pthread::_pthread_rwlock_unlock),
        "_pthread_rwlock_wrlock" => Function::new_env(store, env, crate::pthread::_pthread_rwlock_wrlock),
        "_pthread_setcancelstate" => Function::new_env(store, env, crate::pthread::_pthread_setcancelstate),
        "_pthread_setspecific" => Function::new_env(store, env, crate::pthread::_pthread_setspecific),
        "_pthread_sigmask" => Function::new_env(store, env, crate::pthread::_pthread_sigmask),
        "___gxx_personality_v0" => Function::new_env(store, env, crate::emscripten_target::___gxx_personality_v0),
        "_gai_strerror" => Function::new_env(store, env, crate::env::_gai_strerror),
        "_getdtablesize" => Function::new_env(store, env, crate::emscripten_target::_getdtablesize),
        "_gethostbyaddr" => Function::new_env(store, env, crate::emscripten_target::_gethostbyaddr),
        "_gethostbyname" => Function::new_env(store, env, crate::emscripten_target::_gethostbyname),
        "_gethostbyname_r" => Function::new_env(store, env, crate::emscripten_target::_gethostbyname_r),
        "_getloadavg" => Function::new_env(store, env, crate::emscripten_target::_getloadavg),
        "_getnameinfo" => Function::new_env(store, env, crate::emscripten_target::_getnameinfo),
        "invoke_dii" => Function::new_env(store, env, crate::emscripten_target::invoke_dii),
        "invoke_diiii" => Function::new_env(store, env, crate::emscripten_target::invoke_diiii),
        "invoke_iiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiii),
        "invoke_iiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiii),
        "invoke_iiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiiii),
        "invoke_iiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiiiii),
        "invoke_iiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiiiiii),
        "invoke_iiiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiiiiiii),
        "invoke_iiiiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_iiiiiiiiiii),
        "invoke_vd" => Function::new_env(store, env, crate::emscripten_target::invoke_vd),
        "invoke_viiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiii),
        "invoke_viiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiii),
        "invoke_viiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiiii),
        "invoke_viiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiiiii),
        "invoke_viiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiiiiii),
        "invoke_viiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiiiiii),
        "invoke_viiiiiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiiiiiiiii),
        "invoke_iij" => Function::new_env(store, env, crate::emscripten_target::invoke_iij),
        "invoke_iji" => Function::new_env(store, env, crate::emscripten_target::invoke_iji),
        "invoke_iiji" => Function::new_env(store, env, crate::emscripten_target::invoke_iiji),
        "invoke_iiijj" => Function::new_env(store, env, crate::emscripten_target::invoke_iiijj),
        "invoke_j" => Function::new_env(store, env, crate::emscripten_target::invoke_j),
        "invoke_ji" => Function::new_env(store, env, crate::emscripten_target::invoke_ji),
        "invoke_jii" => Function::new_env(store, env, crate::emscripten_target::invoke_jii),
        "invoke_jij" => Function::new_env(store, env, crate::emscripten_target::invoke_jij),
        "invoke_jjj" => Function::new_env(store, env, crate::emscripten_target::invoke_jjj),
        "invoke_viiij" => Function::new_env(store, env, crate::emscripten_target::invoke_viiij),
        "invoke_viiijiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiijiiii),
        "invoke_viiijiiiiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viiijiiiiii),
        "invoke_viij" => Function::new_env(store, env, crate::emscripten_target::invoke_viij),
        "invoke_viiji" => Function::new_env(store, env, crate::emscripten_target::invoke_viiji),
        "invoke_viijiii" => Function::new_env(store, env, crate::emscripten_target::invoke_viijiii),
        "invoke_viijj" => Function::new_env(store, env, crate::emscripten_target::invoke_viijj),
        "invoke_vij" => Function::new_env(store, env, crate::emscripten_target::invoke_vij),
        "invoke_viji" => Function::new_env(store, env, crate::emscripten_target::invoke_viji),
        "invoke_vijiii" => Function::new_env(store, env, crate::emscripten_target::invoke_vijiii),
        "invoke_vijj" => Function::new_env(store, env, crate::emscripten_target::invoke_vijj),
        "invoke_vidd" => Function::new_env(store, env, crate::emscripten_target::invoke_vidd),
        "invoke_viid" => Function::new_env(store, env, crate::emscripten_target::invoke_viid),
        "invoke_viidii" => Function::new_env(store, env, crate::emscripten_target::invoke_viidii),
        "invoke_viidddddddd" => Function::new_env(store, env, crate::emscripten_target::invoke_viidddddddd),

        // ucontext
        "_getcontext" => Function::new_env(store, env, crate::ucontext::_getcontext),
        "_makecontext" => Function::new_env(store, env, crate::ucontext::_makecontext),
        "_setcontext" => Function::new_env(store, env, crate::ucontext::_setcontext),
        "_swapcontext" => Function::new_env(store, env, crate::ucontext::_swapcontext),

        // unistd
        "_confstr" => Function::new_env(store, env, crate::unistd::confstr),
    };

    // Compatibility with newer versions of Emscripten
    use crate::wasmer_runtime_core::import::LikeNamespace;
    for (k, v) in env_ns.get_exports() {
        if k.starts_with("_") {
            let k = &k[1..];
            if !env_ns.contains_key(k) {
                env_ns.insert(k, v.to_export());
            }
        }
    }

    for null_func_name in globals.null_func_names.iter() {
        env_ns.insert(null_func_name.as_str(), Function::new(nullfunc).to_export());
    }

    let import_object: ImportObject = imports! {
        "env" => env_ns,
        "global" => {
          "NaN" => Global::new(Val::F64(f64::NAN)),
          "Infinity" => Global::new(Val::F64(f64::INFINITY)),
        },
        "global.Math" => {
            "pow" => Function::new(store, crate::math::pow),
            "exp" => Function::new(store, crate::math::exp),
            "log" => Function::new(store, env, crate::math::log),
        },
        "asm2wasm" => {
            "f64-rem" => Function::new_env(store, env, crate::math::f64_rem),
            "f64-to-int" => Function::new_env(store, env, crate::math::f64_to_int),
        },
    };

    import_object
}

pub fn nullfunc(ctx: &mut EmEnv, _x: u32) {
    use crate::process::abort_with_message;
    debug!("emscripten::nullfunc_i {}", _x);
    abort_with_message(
        ctx,
        "Invalid function pointer. Perhaps this is an invalid value \
    (e.g. caused by calling a virtual method on a NULL pointer)? Or calling a function with an \
    incorrect type, which will fail? (it is worth building your source files with -Werror (\
    warnings are errors), as warnings can indicate undefined behavior which can cause this)",
    );
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
