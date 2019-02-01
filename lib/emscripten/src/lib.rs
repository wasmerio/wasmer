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
    use crate::varargs::VarArgs;
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

            // Globals
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

            // IO
            "printf" => func!(crate::io::printf, [i32, i32] -> [i32]),
            "putchar" => func!(crate::io::putchar, [i32] -> []),
            "___lock" => func!(crate::lock::___lock, [i32] -> []),
            "___unlock" => func!(crate::lock::___unlock, [i32] -> []),
            "___wait" => func!(crate::lock::___wait, [u32, u32, u32, u32] -> []),

            // Env
            "___assert_fail" => func!(crate::env::___assert_fail, [i32, i32, i32, i32] -> []),
            "_getenv" => func!(crate::env::_getenv, [i32] -> [u32]),
            "_setenv" => func!(crate::env::_setenv, [i32, i32, i32] -> [i32]),
            "_putenv" => func!(crate::env::_putenv, [i32] -> [i32]),
            "_unsetenv" => func!(crate::env::_unsetenv, [i32] -> [i32]),
            "_getpwnam" => func!(crate::env::_getpwnam, [i32] -> [i32]),
            "_getgrnam" => func!(crate::env::_getgrnam, [i32] -> [i32]),
            "___buildEnvironment" => func!(crate::env::___build_environment, [i32] -> []),
            "___setErrNo" => func!(crate::errno::___seterrno, [i32] -> []),
            "_getpagesize" => func!(crate::env::_getpagesize, [] -> [u32]),
            "_sysconf" => func!(crate::env::_sysconf, [i32] -> [i64]),
            "_getaddrinfo" => func!(crate::env::_getaddrinfo, [i32, i32, i32, i32] -> [i32]),

            // Null func
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

            // Syscalls
            "___syscall1" => func!(crate::syscalls::___syscall1, [i32, VarArgs] -> []),
            "___syscall3" => func!(crate::syscalls::___syscall3, [i32, VarArgs] -> [i32]),
            "___syscall4" => func!(crate::syscalls::___syscall4, [i32, VarArgs] -> [i32]),
            "___syscall5" => func!(crate::syscalls::___syscall5, [i32, VarArgs] -> [i32]),
            "___syscall6" => func!(crate::syscalls::___syscall6, [i32, VarArgs] -> [i32]),
            "___syscall10" => func!(crate::syscalls::___syscall10, [i32, i32] -> [i32]),
            "___syscall12" => func!(crate::syscalls::___syscall12, [i32, VarArgs] -> [i32]),
            "___syscall15" => func!(crate::syscalls::___syscall15, [i32, i32] -> [i32]),
            "___syscall20" => func!(crate::syscalls::___syscall20, [i32, i32] -> [i32]),
            "___syscall39" => func!(crate::syscalls::___syscall39, [i32, VarArgs] -> [i32]),
            "___syscall38" => func!(crate::syscalls::___syscall38, [i32, i32] -> [i32]),
            "___syscall40" => func!(crate::syscalls::___syscall40, [i32, VarArgs] -> [i32]),
            "___syscall54" => func!(crate::syscalls::___syscall54, [i32, VarArgs] -> [i32]),
            "___syscall57" => func!(crate::syscalls::___syscall57, [i32, VarArgs] -> [i32]),
            "___syscall60" => func!(crate::syscalls::___syscall60, [i32, i32] -> [i32]),
            "___syscall63" => func!(crate::syscalls::___syscall63, [i32, VarArgs] -> [i32]),
            "___syscall64" => func!(crate::syscalls::___syscall64, [i32, i32] -> [i32]),
            "___syscall66" => func!(crate::syscalls::___syscall66, [i32, i32] -> [i32]),
            "___syscall75" => func!(crate::syscalls::___syscall75, [i32, i32] -> [i32]),
            "___syscall85" => func!(crate::syscalls::___syscall85, [i32, i32] -> [i32]),
            "___syscall91" => func!(crate::syscalls::___syscall91, [i32, i32] -> [i32]),
            "___syscall97" => func!(crate::syscalls::___syscall97, [i32, i32] -> [i32]),
            "___syscall102" => func!(crate::syscalls::___syscall102, [i32, VarArgs] -> [i32]),
            "___syscall110" => func!(crate::syscalls::___syscall110, [i32, i32] -> [i32]),
            "___syscall114" => func!(crate::syscalls::___syscall114, [i32, VarArgs] -> [i32]),
            "___syscall122" => func!(crate::syscalls::___syscall122, [i32, VarArgs] -> [i32]),
            "___syscall140" => func!(crate::syscalls::___syscall140, [i32, VarArgs] -> [i32]),
            "___syscall142" => func!(crate::syscalls::___syscall142, [i32, VarArgs] -> [i32]),
            "___syscall145" => func!(crate::syscalls::___syscall145, [i32, VarArgs] -> [i32]),
            "___syscall146" => func!(crate::syscalls::___syscall146, [i32, VarArgs] -> [i32]),
            "___syscall168" => func!(crate::syscalls::___syscall168, [i32, i32] -> [i32]),
            "___syscall180" => func!(crate::syscalls::___syscall180, [i32, VarArgs] -> [i32]),
            "___syscall181" => func!(crate::syscalls::___syscall181, [i32, VarArgs] -> [i32]),
            "___syscall191" => func!(crate::syscalls::___syscall191, [i32, i32] -> [i32]),
            "___syscall192" => func!(crate::syscalls::___syscall192, [i32, VarArgs] -> [i32]),
            "___syscall194" => func!(crate::syscalls::___syscall194, [i32, i32] -> [i32]),
            "___syscall195" => func!(crate::syscalls::___syscall195, [i32, VarArgs] -> [i32]),
            "___syscall196" => func!(crate::syscalls::___syscall196, [i32, i32] -> [i32]),
            "___syscall197" => func!(crate::syscalls::___syscall197, [i32, VarArgs] -> [i32]),
            "___syscall199" => func!(crate::syscalls::___syscall199, [i32, i32] -> [i32]),
            "___syscall201" => func!(crate::syscalls::___syscall201, [i32, i32] -> [i32]),
            "___syscall202" => func!(crate::syscalls::___syscall202, [i32, i32] -> [i32]),
            "___syscall212" => func!(crate::syscalls::___syscall212, [i32, VarArgs] -> [i32]),
            "___syscall220" => func!(crate::syscalls::___syscall220, [i32, i32] -> [i32]),
            "___syscall221" => func!(crate::syscalls::___syscall221, [i32, VarArgs] -> [i32]),
            "___syscall268" => func!(crate::syscalls::___syscall268, [i32, i32] -> [i32]),
            "___syscall272" => func!(crate::syscalls::___syscall272, [i32, i32] -> [i32]),
            "___syscall295" => func!(crate::syscalls::___syscall295, [i32, i32] -> [i32]),
            "___syscall300" => func!(crate::syscalls::___syscall300, [i32, i32] -> [i32]),
            "___syscall330" => func!(crate::syscalls::___syscall330, [i32, VarArgs] -> [i32]),
            "___syscall334" => func!(crate::syscalls::___syscall334, [i32, i32] -> [i32]),
            "___syscall340" => func!(crate::syscalls::___syscall340, [i32, VarArgs] -> [i32]),

            // Process
            "abort" => func!(crate::process::em_abort, [u32] -> []),
            "_abort" => func!(crate::process::_abort, [] -> []),
            "abortStackOverflow" => func!(crate::process::abort_stack_overflow, [i32] -> []),
            "_llvm_trap" => func!(crate::process::_llvm_trap, [] -> []),
            "_fork" => func!(crate::process::_fork, [] -> [i32]),
            "_exit" => func!(crate::process::_exit, [i32] -> []),
            "_system" => func!(crate::process::_system, [i32] -> [i32]),
            "_popen" => func!(crate::process::_popen, [i32, i32] -> [i32]),
            "_endgrent" => func!(crate::process::_endgrent, [] -> []),
            "_execve" => func!(crate::process::_execve, [i32, i32, i32] -> [i32]),
            "_kill" => func!(crate::process::_kill, [i32, i32] -> [i32]),
            "_llvm_stackrestore" => func!(crate::process::_llvm_stackrestore, [i32] -> []),
            "_raise" => func!(crate::process::_raise, [i32] -> [i32]),
            "_sem_init" => func!(crate::process::_sem_init, [i32, i32, i32] -> [i32]),
            "_sem_post" => func!(crate::process::_sem_post, [i32] -> [i32]),
            "_sem_wait" => func!(crate::process::_sem_wait, [i32] -> [i32]),
            "_setgrent" => func!(crate::process::_setgrent, [] -> []),
            "_setgroups" => func!(crate::process::_setgroups, [i32, i32] -> [i32]),
            "_setitimer" => func!(crate::process::_setitimer, [i32, i32, i32] -> [i32]),
            "_usleep" => func!(crate::process::_usleep, [i32] -> [i32]),
            "_utimes" => func!(crate::process::_utimes, [i32, i32] -> [i32]),
            "_waitpid" => func!(crate::process::_waitpid, [i32, i32, i32] -> [i32]),

            // Signal
            "_sigemptyset" => func!(crate::signal::_sigemptyset, [u32] -> [i32]),
            "_sigaddset" => func!(crate::signal::_sigaddset, [u32, u32] -> [i32]),
            "_sigprocmask" => func!(crate::signal::_sigprocmask, [i32, i32, i32] -> [i32]),
            "_sigaction" => func!(crate::signal::_sigaction, [u32, u32, u32] -> [i32]),
            "_signal" => func!(crate::signal::_signal, [u32, i32] -> [i32]),
            "_sigsuspend" => func!(crate::signal::_sigsuspend, [i32] -> [i32]),

            // Memory
            "abortOnCannotGrowMemory" => func!(crate::memory::abort_on_cannot_grow_memory, [] -> [u32]),
            "_emscripten_memcpy_big" => func!(crate::memory::_emscripten_memcpy_big, [u32, u32, u32] -> [u32]),
            "enlargeMemory" => func!(crate::memory::enlarge_memory, [] -> [u32]),
            "getTotalMemory" => func!(crate::memory::get_total_memory, [] -> [u32]),
            "___map_file" => func!(crate::memory::___map_file, [u32, u32] -> [i32]),

            // Exception
            "___cxa_allocate_exception" => func!(crate::exception::___cxa_allocate_exception, [u32] -> [u32]),
            "___cxa_throw" => func!(crate::exception::___cxa_throw, [u32, u32, u32] -> []),

            // Time
            "_gettimeofday" => func!(crate::time::_gettimeofday, [i32, i32] -> [i32]),
            "_clock_gettime" => func!(crate::time::_clock_gettime, [u32, i32] -> [i32]),
            "___clock_gettime" => func!(crate::time::_clock_gettime, [u32, i32] -> [i32]),
            "_clock" => func!(crate::time::_clock, [] -> [i32]),
            "_difftime" => func!(crate::time::_difftime, [u32, u32] -> [f64]),
            "_asctime" => func!(crate::time::_asctime, [u32] -> [u32]),
            "_asctime_r" => func!(crate::time::_asctime_r, [u32, u32] -> [u32]),
            "_localtime" => func!(crate::time::_localtime, [u32] -> [i32]),
            "_time" => func!(crate::time::_time, [u32] -> [i64]),
            "_strftime" => func!(crate::time::_strftime, [i32, u32, i32, i32] -> [i64]),
            "_localtime_r" => func!(crate::time::_localtime_r, [u32, u32] -> [i32]),
            "_gmtime_r" => func!(crate::time::_gmtime_r, [i32, i32] -> [i32]),
            "_mktime" => func!(crate::time::_mktime, [i32] -> [i32]),
            "_gmtime" => func!(crate::time::_gmtime, [i32] -> [i32]),

            // Math
            "f64-rem" => func!(crate::math::f64_rem, [f64, f64] -> [f64]),
            "_llvm_log10_f64" => func!(crate::math::_llvm_log10_f64, [f64] -> [f64]),
            "_llvm_log2_f64" => func!(crate::math::_llvm_log2_f64, [f64] -> [f64]),
            "_llvm_log10_f32" => func!(crate::math::_llvm_log10_f32, [f64] -> [f64]),
            "_llvm_log2_f32" => func!(crate::math::_llvm_log2_f64, [f64] -> [f64]),
            "_emscripten_random" => func!(crate::math::_emscripten_random, [] -> [f64]),

            // Jump
            "__setjmp" => func!(crate::jmp::__setjmp, [u32] -> [i32]),
            "__longjmp" => func!(crate::jmp::__longjmp, [u32, i32] -> []),

            // Linking
            "_dlclose" => func!(crate::linking::_dlclose, [u32] -> [i32]),
            "_dlopen" => func!(crate::linking::_dlopen, [u32, u32] -> [i32]),
            "_dlsym" => func!(crate::linking::_dlopen, [u32, u32] -> [i32]),

        },
        "math" => {
            "pow" => func!(crate::math::pow, [f64, f64] -> [f64]),
        },
    };
    // mock_external!(env_namespace, _sched_yield);
    // mock_external!(env_namespace, _llvm_stacksave);
    // mock_external!(env_namespace, _getgrent);
    // mock_external!(env_namespace, _dlerror);

    imports.register("env", env_namespace);
    imports.register("asm2wasm", asm_namespace);
    imports.register("global", global_namespace);
    imports.register("global.Math", global_math_namespace);

    imports
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
