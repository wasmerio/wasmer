#[macro_use]
extern crate wasmer_runtime_core;

use byteorder::{ByteOrder, LittleEndian};
use libc::c_int;
use std::cell::UnsafeCell;
use std::{f64, ffi::c_void, fmt, mem, ptr};
use wasmer_runtime_core::{
    error::CallResult,
    export::{Context, Export, FuncPointer},
    func,
    global::Global,
    import::{ImportObject, Namespace},
    imports,
    memory::Memory,
    table::Table,
    types::{
        ElementType, FuncSig, GlobalDescriptor, MemoryDescriptor, TableDescriptor,
        Type::{self, *},
        Value,
    },
    units::Pages,
    vm::Ctx,
    vm::LocalGlobal,
    vm::LocalMemory,
    vm::LocalTable,
    Instance, Module,
};

#[macro_use]
mod macros;
//#[cfg(test)]
mod file_descriptor;
pub mod stdio;

// EMSCRIPTEN APIS
mod env;
mod errno;
mod exception;
mod io;
mod jmp;
mod linking;
mod lock;
mod math;
mod memory;
mod nullfunc;
mod process;
mod signal;
mod storage;
mod syscalls;
mod time;
mod utils;
mod varargs;

pub use self::storage::align_memory;
pub use self::utils::{
    allocate_cstr_on_stack, allocate_on_stack, get_emscripten_memory_size,
    get_emscripten_table_size, is_emscripten_module,
};

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5_242_880;
// TODO: Magic number - how is this calculated?
const DYNAMICTOP_PTR_DIFF: u32 = 1088;
// TODO: make this variable
const STATIC_BUMP: u32 = 215_536;

// The address globals begin at. Very low in memory, for code size and optimization opportunities.
// Above 0 is static memory, starting with globals.
// Then the stack.
// Then 'dynamic' memory for sbrk.
const GLOBAL_BASE: u32 = 1024;
const STATIC_BASE: i32 = GLOBAL_BASE as i32;

fn stacktop(static_bump: u32) -> u32 {
    align_memory(dynamictop_ptr(static_bump) + 4)
}

fn stack_max(static_bump: u32) -> u32 {
    stacktop(static_bump) + TOTAL_STACK
}

fn dynamic_base(static_bump: u32) -> u32 {
    align_memory(stack_max(static_bump))
}

fn dynamictop_ptr(static_bump: u32) -> u32 {
    static_bump + DYNAMICTOP_PTR_DIFF
}

pub struct EmscriptenData {
    pub malloc: extern "C" fn(i32, &mut Ctx) -> u32,
    pub free: extern "C" fn(i32, &mut Ctx),
    pub memalign: extern "C" fn(u32, u32, &mut Ctx) -> u32,
    pub memset: extern "C" fn(u32, i32, u32, &mut Ctx) -> u32,
    pub stack_alloc: extern "C" fn(u32, &mut Ctx) -> u32,
    pub jumps: Vec<UnsafeCell<[c_int; 27]>>,
}

impl EmscriptenData {
    pub fn new(instance: &mut Instance) -> Self {
        unsafe {
            let malloc_func = instance.func("_malloc");
            let malloc_addr = if let Ok(malloc_func) = malloc_func {
                malloc_func.raw() as *const u8
            } else {
                0 as *const u8
            };
            let free_func = instance.func("_free");
            let free_addr = if let Ok(free_func) = free_func {
                free_func.raw() as *const u8
            } else {
                0 as *const u8
            };
            let memalign_func = instance.func("_memalign");
            let memalign_addr = if let Ok(memalign_func) = memalign_func {
                memalign_func.raw() as *const u8
            } else {
                0 as *const u8
            };
            let memset_func = instance.func("_memset");
            let memset_addr = if let Ok(memset_func) = memset_func {
                memset_func.raw() as *const u8
            } else {
                0 as *const u8
            };
            let stack_alloc_func = instance.func("stackAlloc");
            let stack_alloc_addr = if let Ok(stack_alloc_func) = stack_alloc_func {
                stack_alloc_func.raw() as *const u8
            } else {
                0 as *const u8
            };

            EmscriptenData {
                malloc: mem::transmute(malloc_addr),
                free: mem::transmute(free_addr),
                memalign: mem::transmute(memalign_addr),
                memset: mem::transmute(memset_addr),
                stack_alloc: mem::transmute(stack_alloc_addr),
                jumps: Vec::new(),
            }
        }
    }
}

impl fmt::Debug for EmscriptenData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("EmscriptenData")
            .field("malloc", &(self.malloc as usize))
            .field("free", &(self.free as usize))
            .finish()
    }
}

pub fn run_emscripten_instance(
    _module: &Module,
    instance: &mut Instance,
    path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    let mut data = EmscriptenData::new(instance);
    let data_ptr = &mut data as *mut _ as *mut c_void;
    instance.context_mut().data = data_ptr;

    let main_func = instance.func("_main")?;
    let num_params = main_func.signature().params().len();
    let _result = match num_params {
        2 => {
            let (argc, argv) = store_module_arguments(path, args, instance.context_mut());
            instance.call("_main", &[Value::I32(argc as i32), Value::I32(argv as i32)])?;
        }
        0 => {
            instance.call("_main", &[])?;
        }
        _ => panic!(
            "The emscripten main function has received an incorrect number of params {}",
            num_params
        ),
    };

    // TODO atinit and atexit for emscripten
    println!("{:?}", data);
    Ok(())
}

fn store_module_arguments(path: &str, args: Vec<&str>, ctx: &mut Ctx) -> (u32, u32) {
    let argc = args.len() + 1;

    let mut args_slice = vec![0; argc];
    args_slice[0] = unsafe { allocate_cstr_on_stack(path, ctx).0 };
    for (slot, arg) in args_slice[1..argc].iter_mut().zip(args.iter()) {
        *slot = unsafe { allocate_cstr_on_stack(&arg, ctx).0 };
    }

    let (argv_offset, argv_slice): (_, &mut [u32]) =
        unsafe { allocate_on_stack(((argc + 1) * 4) as u32, ctx) };
    assert!(!argv_slice.is_empty());
    for (slot, arg) in argv_slice[0..argc].iter_mut().zip(args_slice.iter()) {
        *slot = *arg
    }
    argv_slice[argc] = 0;

    (argc as u32, argv_offset)
}

pub fn emscripten_set_up_memory(memory: &mut Memory) {
    let dynamictop_ptr = dynamictop_ptr(STATIC_BUMP) as usize;
    let dynamictop_ptr_offset = dynamictop_ptr + mem::size_of::<u32>();

    // println!("value = {:?}");

    // We avoid failures of setting the u32 in our memory if it's out of bounds
    unimplemented!()
    //    if dynamictop_ptr_offset > memory.len() {
    //        return; // TODO: We should panic instead?
    //    }
    //
    //    // debug!("###### dynamic_base = {:?}", dynamic_base(STATIC_BUMP));
    //    // debug!("###### dynamictop_ptr = {:?}", dynamictop_ptr);
    //    // debug!("###### dynamictop_ptr_offset = {:?}", dynamictop_ptr_offset);
    //
    //    let mem = &mut memory[dynamictop_ptr..dynamictop_ptr_offset];
    //    LittleEndian::write_u32(mem, dynamic_base(STATIC_BUMP));
}

macro_rules! mock_external {
    ($namespace:ident, $name:ident) => {{
        extern "C" fn _mocked_fn() -> i32 {
            debug!("emscripten::{} <mock>", stringify!($name));
            -1
        }

        $namespace.insert(
            stringify!($name),
            Export::Function {
                func: unsafe { FuncPointer::new(_mocked_fn as _) },
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );
    }};
}

macro_rules! global {
    ($value:expr) => {{
        unsafe {
            GlobalPointer::new(
                // NOTE: Taking a shortcut here. LocalGlobal is a struct containing just u64.
                std::mem::transmute::<&u64, *mut LocalGlobal>(&$value),
            )
        }
    }};
}

pub struct EmscriptenGlobalsData {
    abort: u64,
    // Env namespace
    stacktop: u64,
    stack_max: u64,
    dynamictop_ptr: u64,
    memory_base: u64,
    table_base: u64,
    temp_double_ptr: u64,

    // Global namespace
    infinity: u64,
    nan: u64,
}

pub struct EmscriptenGlobals {
    // The emscripten data
    pub data: EmscriptenGlobalsData,
    // The emscripten memory
    pub memory: Memory,
    pub table: Table,
    pub memory_min: Pages,
    pub memory_max: Option<Pages>,
}

impl EmscriptenGlobals {
    pub fn new(module: &Module) -> Self {
        let (table_min, table_max) = get_emscripten_table_size(&module);
        let (memory_min, memory_max) = get_emscripten_memory_size(&module);

        // Memory initialization
        let memory_type = MemoryDescriptor {
            minimum: memory_min,
            maximum: memory_max,
            shared: false,
        };
        let mut memory = Memory::new(memory_type).unwrap();

        let table_type = TableDescriptor {
            element: ElementType::Anyfunc,
            minimum: table_min,
            maximum: table_max,
        };
        let mut table = Table::new(table_type).unwrap();

        let memory_base = STATIC_BASE as u64;
        let table_base = 0 as u64;
        let temp_double_ptr = 0 as u64;
        let data = EmscriptenGlobalsData {
            abort: 0, // TODO review usage
            // env
            stacktop: stacktop(STATIC_BUMP) as _,
            stack_max: stack_max(STATIC_BUMP) as _,
            dynamictop_ptr: dynamictop_ptr(STATIC_BUMP) as _,
            memory_base: memory_base,
            table_base: table_base,
            temp_double_ptr: temp_double_ptr,

            // global
            infinity: std::f64::INFINITY.to_bits() as _,
            nan: std::f64::NAN.to_bits() as _,
        };

        Self {
            data,
            memory,
            table,
            memory_min,
            memory_max,
        }
    }
}

pub fn generate_emscripten_env(globals: &mut EmscriptenGlobals) -> ImportObject {
    let mut imports = ImportObject::new();
    let mut env_namespace = Namespace::new();
    let mut asm_namespace = Namespace::new();
    let mut global_namespace = Namespace::new();
    let mut global_math_namespace = Namespace::new();

    // Add globals.
    // NOTE: There is really no need for checks, these globals should always be available.

    // We generate a fake Context that traps on access
    let null_ctx = Context::External(ptr::null_mut());

    //    env_namespace.insert("memory".to_string(), Export::Memory(globals.memory.clone()));

    //    env_namespace.insert("table".to_string(), Export::Table(globals.table.clone()));

    let import_object = imports! {
            "env" => {
                "memory" => Export::Memory(globals.memory.clone()),
                "table" => Export::Table(globals.table.clone()),
                // Globals.
                "STACKTOP" => Global::new(Value::I32(stacktop(STATIC_BUMP) as i32)),
                            "STACK_MAX" => Global::new(Value::I32(stack_max(STATIC_BUMP) as i32)),
                    "DYNAMICTOP_PTR" => Global::new(Value::I32(dynamictop_ptr(STATIC_BUMP) as i32)),
                    "tableBase" => Global::new(Value::I32(0)),
                    "__table_base" => Global::new(Value::I32(0)),
                    "Infinity" => Global::new(Value::F64(f64::INFINITY)),
                    "NaN" => Global::new(Value::F64(f64::NAN)),
                     "ABORT" => Global::new(Value::I32(0)),
                     "memoryBase" => Global::new(Value::I32(STATIC_BASE)),
                     "__memory_base" => Global::new(Value::I32(STATIC_BASE)),
                     "tempDoublePtr" => Global::new(Value::I32(0)),
                     "printf" => func!(crate::io::printf, [i32, i32] -> [i32]),
                     "putchar" => func!(crate::io::putchar, [i32] -> []),
                     "___assert_fail" => func!(crate::env::___assert_fail, [i32, i32, i32, i32] -> []),
                     "___lock" => func!(crate::lock::___lock, [i32] -> []),
                     "___unlock" => func!(crate::lock::___unlock, [i32] -> []),
                     "___wait" => func!(crate::lock::___wait, [u32, u32, u32, u32] -> []),
                     "_getenv" => func!(crate::env::_getenv, [i32] -> [u32]),
                     "_setenv" => func!(crate::env::_setenv, [i32, i32, i32] -> [i32]),
                     "_putenv" => func!(crate::env::_putenv, [i32] -> [i32]),
                     "_unsetenv" => func!(crate::env::_unsetenv, [i32] -> [i32]),
                     "_getpwnam" => func!(crate::env::_getpwnam, [i32] -> [i32]),
                     "_getgrnam" => func!(crate::env::_getgrnam, [i32] -> [i32]),
                     "___buildEnvironment" => func!(crate::env::___build_environment, [i32] -> []),
                     "___setErrNo" => func!(crate::errno::___seterrno, [i32] -> []),
    //                  "___syscall1" => func!(crate::syscalls::___syscall1, [i32, i32] -> []),
                    "nullFunc_i" => func!(crate::nullfunc::nullfunc_i, [u32] -> []),
                    "nullFunc_ii" => func!(crate::nullfunc::nullfunc_ii, [u32] -> []),
                     "nullFunc_iii" => func!(crate::nullfunc::nullfunc_iii, [u32] -> []),
                     "nullFunc_iiii" => func!(crate::nullfunc::nullfunc_iiii, [u32] -> []),
                     "nullFunc_iiiii" => func!(crate::nullfunc::nullfunc_iiiii, [u32] -> []),
                     "nullFunc_iiiiii" => func!(crate::nullfunc::nullfunc_iiiiii, [u32] -> []),
                     "nullFunc_v" => func!(crate::nullfunc::nullfunc_v, [u32] -> []),
                     "nullFunc_vi" => func!(crate::nullfunc::nullfunc_vi, [u32] -> []),
                     "nullFunc_vii" => func!(crate::nullfunc::nullfunc_vii, [u32] -> []),
                     "nullFunc_viii" => func!(crate::nullfunc::nullfunc_viii, [u32] -> []),
                     "nullFunc_viiii" => func!(crate::nullfunc::nullfunc_viiii, [u32] -> []),
                     "nullFunc_viiiii" => func!(crate::nullfunc::nullfunc_viiiii, [u32] -> []),
                     "nullFunc_viiiiii" => func!(crate::nullfunc::nullfunc_viiiiii, [u32] -> []),
            },
            "math" => {
                "pow" => func!(crate::math::pow, [f64, f64] -> [f64]),
            },
        };

    //    // Syscalls
    //    env_namespace.insert(
    //        "___syscall1",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall1),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![],
    //            },
    //        },
    //    );

    //
    //    env_namespace.insert(
    //        "___syscall3",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall3),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall4",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall4),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall5",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall5),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall6",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall6),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall12",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall12),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall20",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall20),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall220",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall220),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall39",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall39),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall40",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall40),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall10",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall10),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall54",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall54),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall57",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall57),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall63",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall63),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall85",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall85),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall64",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall64),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall102",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall102),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall114",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall114),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall122",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall122),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall140",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall140),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall142",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall142),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall145",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall145),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall146",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall146),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall180",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall180),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall181",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall181),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall192",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall192),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall195",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall195),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall197",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall197),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall201",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall201),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall202",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall202),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall212",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall212),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall221",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall221),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall330",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall330),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall340",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall340),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    // Process
    //    env_namespace.insert(
    //        "abort",
    //        Export::Function {
    //            func: func!(process, em_abort),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_abort",
    //        Export::Function {
    //            func: func!(process, _abort),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "abortStackOverflow",
    //        Export::Function {
    //            func: func!(process, abort_stack_overflow),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_trap",
    //        Export::Function {
    //            func: func!(process, _llvm_trap),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_fork",
    //        Export::Function {
    //            func: func!(process, _fork),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_exit",
    //        Export::Function {
    //            func: func!(process, _exit),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_system",
    //        Export::Function {
    //            func: func!(process, _system),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_popen",
    //        Export::Function {
    //            func: func!(process, _popen),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    // Signal
    //    env_namespace.insert(
    //        "_sigemptyset",
    //        Export::Function {
    //            func: func!(signal, _sigemptyset),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sigaddset",
    //        Export::Function {
    //            func: func!(signal, _sigaddset),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sigprocmask",
    //        Export::Function {
    //            func: func!(signal, _sigprocmask),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sigaction",
    //        Export::Function {
    //            func: func!(signal, _sigaction),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_signal",
    //        Export::Function {
    //            func: func!(signal, _signal),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    // Memory
    //    env_namespace.insert(
    //        "abortOnCannotGrowMemory",
    //        Export::Function {
    //            func: func!(memory, abort_on_cannot_grow_memory),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_emscripten_memcpy_big",
    //        Export::Function {
    //            func: func!(memory, _emscripten_memcpy_big),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "enlargeMemory",
    //        Export::Function {
    //            func: func!(memory, enlarge_memory),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "getTotalMemory",
    //        Export::Function {
    //            func: func!(memory, get_total_memory),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___map_file",
    //        Export::Function {
    //            func: func!(memory, ___map_file),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    // Exception
    //    env_namespace.insert(
    //        "___cxa_allocate_exception",
    //        Export::Function {
    //            func: func!(exception, ___cxa_allocate_exception),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___cxa_allocate_exception",
    //        Export::Function {
    //            func: func!(exception, ___cxa_throw),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___cxa_throw",
    //        Export::Function {
    //            func: func!(exception, ___cxa_throw),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //    // Time
    //    env_namespace.insert(
    //        "_gettimeofday",
    //        Export::Function {
    //            func: func!(time, _gettimeofday),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_clock_gettime",
    //        Export::Function {
    //            func: func!(time, _clock_gettime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___clock_gettime",
    //        Export::Function {
    //            func: func!(time, _clock_gettime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_clock",
    //        Export::Function {
    //            func: func!(time, _clock),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_difftime",
    //        Export::Function {
    //            func: func!(time, _difftime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_asctime",
    //        Export::Function {
    //            func: func!(time, _asctime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_asctime_r",
    //        Export::Function {
    //            func: func!(time, _asctime_r),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_localtime",
    //        Export::Function {
    //            func: func!(time, _localtime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_time",
    //        Export::Function {
    //            func: func!(time, _time),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_strftime",
    //        Export::Function {
    //            func: func!(time, _strftime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_localtime_r",
    //        Export::Function {
    //            func: func!(time, _localtime_r),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_getpagesize",
    //        Export::Function {
    //            func: func!(env, _getpagesize),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sysconf",
    //        Export::Function {
    //            func: func!(env, _sysconf),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    // Math
    //    asm_namespace.insert(
    //        "f64-rem",
    //        Export::Function {
    //            func: func!(math, f64_rem),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![F64, F64],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_log10_f64",
    //        Export::Function {
    //            func: func!(math, _llvm_log10_f64),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![F64],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_log2_f64",
    //        Export::Function {
    //            func: func!(math, _llvm_log2_f64),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![F64],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    //
    //    env_namespace.insert(
    //        "__setjmp",
    //        Export::Function {
    //            func: func!(jmp, __setjmp),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "__longjmp",
    //        Export::Function {
    //            func: func!(jmp, __longjmp),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall110",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall110),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall15",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall15),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall168",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall168),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall191",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall191),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    env_namespace.insert(
    //        "___syscall194",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall194),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    env_namespace.insert(
    //        "___syscall196",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall196),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //    env_namespace.insert(
    //        "___syscall199",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall199),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall268",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall268),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall272",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall272),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall295",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall295),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall300",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall300),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall334",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall334),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall38",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall38),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall60",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall60),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall66",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall66),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall75",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall75),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall91",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall91),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "___syscall97",
    //        Export::Function {
    //            func: func!(syscalls, ___syscall97),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_endgrent",
    //        Export::Function {
    //            func: func!(process, _endgrent),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_execve",
    //        Export::Function {
    //            func: func!(process, _execve),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_getaddrinfo",
    //        Export::Function {
    //            func: func!(env, _getaddrinfo),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_gmtime_r",
    //        Export::Function {
    //            func: func!(time, _gmtime_r),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_kill",
    //        Export::Function {
    //            func: func!(process, _kill),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_stackrestore",
    //        Export::Function {
    //            func: func!(process, _llvm_stackrestore),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_mktime",
    //        Export::Function {
    //            func: func!(time, _mktime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_raise",
    //        Export::Function {
    //            func: func!(process, _raise),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sem_init",
    //        Export::Function {
    //            func: func!(process, _sem_init),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sem_post",
    //        Export::Function {
    //            func: func!(process, _sem_post),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_sem_wait",
    //        Export::Function {
    //            func: func!(process, _sem_wait),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_setgrent",
    //        Export::Function {
    //            func: func!(process, _setgrent),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_setgroups",
    //        Export::Function {
    //            func: func!(process, _setgroups),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_setitimer",
    //        Export::Function {
    //            func: func!(process, _setitimer),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    //
    //    env_namespace.insert(
    //        "_sigsuspend",
    //        Export::Function {
    //            func: func!(signal, _sigsuspend),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_setitimer",
    //        Export::Function {
    //            func: func!(process, _setitimer),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_usleep",
    //        Export::Function {
    //            func: func!(process, _usleep),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_utimes",
    //        Export::Function {
    //            func: func!(process, _utimes),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_waitpid",
    //        Export::Function {
    //            func: func!(process, _waitpid),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_dlclose",
    //        Export::Function {
    //            func: func!(linking, _dlclose),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_dlopen",
    //        Export::Function {
    //            func: func!(linking, _dlopen),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_dlsym",
    //        Export::Function {
    //            func: func!(linking, _dlsym),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32, I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_log10_f32",
    //        Export::Function {
    //            func: func!(math, _llvm_log10_f32),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![F64],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_llvm_log2_f32",
    //        Export::Function {
    //            func: func!(math, _llvm_log2_f32),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![F64],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_emscripten_random",
    //        Export::Function {
    //            func: func!(math, _emscripten_random),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![],
    //                returns: vec![F64],
    //            },
    //        },
    //    );
    //
    //    env_namespace.insert(
    //        "_gmtime",
    //        Export::Function {
    //            func: func!(time, _gmtime),
    //            ctx: Context::Internal,
    //            signature: FuncSig {
    //                params: vec![I32],
    //                returns: vec![I32],
    //            },
    //        },
    //    );
    //
    //    // mock_external!(env_namespace, _time);
    //    // mock_external!(env_namespace, _sysconf);
    //    // mock_external!(env_namespace, _strftime);
    //    // mock_external!(env_namespace, _sigprocmask);
    //    // mock_external!(env_namespace, _sigemptyset);
    //    // mock_external!(env_namespace, _sigaddset);
    //    // mock_external!(env_namespace, _sigaction);
    //
    //    mock_external!(env_namespace, _sched_yield);
    //    // mock_external!(env_namespace, _localtime_r);
    //    // mock_external!(env_namespace, _localtime);
    //    mock_external!(env_namespace, _llvm_stacksave);
    //    // mock_external!(env_namespace, _gettimeofday);
    //    // mock_external!(env_namespace, _getpagesize);
    //    mock_external!(env_namespace, _getgrent);
    //    // mock_external!(env_namespace, _fork);
    //    // mock_external!(env_namespace, _exit);
    //    // mock_external!(env_namespace, _clock_gettime);
    //    // mock_external!(env_namespace, ___syscall64);
    //    // mock_external!(env_namespace, ___syscall63);
    //    // mock_external!(env_namespace, ___syscall60);
    //    // mock_external!(env_namespace, ___syscall54);
    //    // mock_external!(env_namespace, ___syscall39);
    //    // mock_external!(env_namespace, ___syscall340);
    //    // mock_external!(env_namespace, ___syscall221);
    //    // mock_external!(env_namespace, ___syscall212);
    //    // mock_external!(env_namespace, ___syscall201);
    //    // mock_external!(env_namespace, ___syscall197);
    //    // mock_external!(env_namespace, ___syscall195);
    //    // mock_external!(env_namespace, ___syscall181);
    //    // mock_external!(env_namespace, ___syscall180);
    //    // mock_external!(env_namespace, ___syscall146);
    //    // mock_external!(env_namespace, ___syscall145);
    //    // mock_external!(env_namespace, ___syscall142);
    //    // mock_external!(env_namespace, ___syscall140);
    //    // mock_external!(env_namespace, ___syscall122);
    //    // mock_external!(env_namespace, ___syscall102);
    //    // mock_external!(env_namespace, ___syscall20);
    //    mock_external!(env_namespace, _dlerror);
    //    mock_external!(env_namespace, _gmtime);

    imports.register("env", env_namespace);
    imports.register("asm2wasm", asm_namespace);
    imports.register("global", global_namespace);
    imports.register("global.Math", global_math_namespace);

    imports
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
