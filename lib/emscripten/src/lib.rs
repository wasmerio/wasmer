#[macro_use]
extern crate wasmer_runtime_core;
extern crate wasmer_runtime;

use byteorder::{ByteOrder, LittleEndian};
use hashbrown::HashMap;
use std::mem;
use wasmer_runtime_core::{
    export::{Context, Export, FuncPointer, GlobalPointer},
    import::{ImportObject, Namespace},
    memory::Memory,
    types::{
        FuncSig, GlobalDescriptor,
        Type::{self, *},
    },
    vm::LocalGlobal,
};
use wasmer_runtime::{
    Memory,
    Table,
    Global,
    Value,
    imports,
    func,
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
pub use self::utils::{allocate_cstr_on_stack, allocate_on_stack, is_emscripten_module};

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5_242_880;
// TODO: Magic number - how is this calculated?
const DYNAMICTOP_PTR_DIFF: u32 = 1088;
// TODO: make this variable
const STATIC_BUMP: u32 = 215_536;

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

//pub struct EmscriptenData {
//    pub malloc: extern "C" fn(i32, &Instance) -> u32,
//    pub free: extern "C" fn(i32, &mut Instance),
//    pub memalign: extern "C" fn(u32, u32, &mut Instance) -> u32,
//    pub memset: extern "C" fn(u32, i32, u32, &mut Instance) -> u32,
//    pub stack_alloc: extern "C" fn(u32, &Instance) -> u32,
//    pub jumps: Vec<UnsafeCell<[c_int; 27]>>,
//}

pub fn emscripten_set_up_memory(memory: &mut Memory) {
    let dynamictop_ptr = dynamictop_ptr(STATIC_BUMP) as usize;
    let dynamictop_ptr_offset = dynamictop_ptr + mem::size_of::<u32>();

    // println!("value = {:?}");

    // We avoid failures of setting the u32 in our memory if it's out of bounds
    if dynamictop_ptr_offset > memory.len() {
        return; // TODO: We should panic instead?
    }

    // debug!("###### dynamic_base = {:?}", dynamic_base(STATIC_BUMP));
    // debug!("###### dynamictop_ptr = {:?}", dynamictop_ptr);
    // debug!("###### dynamictop_ptr_offset = {:?}", dynamictop_ptr_offset);

    let mem = &mut memory[dynamictop_ptr..dynamictop_ptr_offset];
    LittleEndian::write_u32(mem, dynamic_base(STATIC_BUMP));
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

pub struct EmscriptenGlobals<'a> {
    pub data: HashMap<&'a str, HashMap<&'a str, (u64, Type)>>, // <namespace, <field_name, (global_value, type)>>
}

impl<'a> EmscriptenGlobals<'a> {
    pub fn new() -> Self {
        let mut data = HashMap::new();
        let mut env_namepace = HashMap::new();
        let mut global_namepace = HashMap::new();

        env_namepace.insert("STACKTOP", (stacktop(STATIC_BUMP) as _, I32));
        env_namepace.insert("STACK_MAX", (stack_max(STATIC_BUMP) as _, I32));
        env_namepace.insert("DYNAMICTOP_PTR", (dynamictop_ptr(STATIC_BUMP) as _, I32));
        env_namepace.insert("tableBase", (0, I32));
        global_namepace.insert("Infinity", (std::f64::INFINITY.to_bits() as _, F64));
        global_namepace.insert("NaN", (std::f64::NAN.to_bits() as _, F64));

        data.insert("env", env_namepace);
        data.insert("global", global_namepace);

        Self { data }
    }
}

pub fn generate_emscripten_env(globals: &EmscriptenGlobals) -> ImportObject {
    let mut imports = ImportObject::new();
    let mut env_namespace = Namespace::new();
    let mut asm_namespace = Namespace::new();
    let mut global_namespace = Namespace::new();

    let import_object = imports! {
        "env" => {
            // Globals.
            "STACKTOP" => Global::new(Value::I32(stacktop(STATIC_BUMP) as i32)),
            "STACK_MAX" => Global::new(Value::I32(stack_max(STATIC_BUMP) as i32)),
            "DYNAMICTOP_PTR" => Global::new(Value::I32(dynamictop_ptr(STATIC_BUMP) as i32)),
            "tableBase" => Global::new(Value::I32(0)),
            "Infinity" => Global::new(Value::F64(f64::INFINITY)),
            "NaN" => Global::new(Value::F64(f64::NAN)),

            // Functions.
            "printf" => func!(self::io::printf, [i32, i32] -> [i32]),
            "putchar" => func!(self::io::putchar, [i32] -> []),

            "___lock" => func!(self::lock::___lock, [i32, i32] -> []),
            "___unlock" => func!(self::lock::___unlock, [i32, i32] -> []),
            "___wait" => func!(self::lock::___wait, [i32, i32] -> []),

            "_getenv" => func!(self::env::_getenv, [i32] -> [i32]),
            "_setenv" => func!(self::env::_setenv, [i32, i32, i32] -> []),
            "_putenv" => func!(self::env::_putenv, [i32] -> []),
            "_unsetenv" => func!(self::env::_unsetenv, [i32] -> []),
            "_getpwnam" => func!(self::env::_getpwnam, [i32] -> [i32]),
            "_getgrnam" => func!(self::env::_getgrnam, [i32] -> [i32]),
            "___buildEnvironment" => func!(self::env::___buildEnvironment, [i32] -> []),

            "___setErrNo" => func!(self::errno::___seterrno, [i32] -> [i32]),

            "___syscall1" => func!(self::syscalls::___syscall1, [i32, i32] -> []),
            "___syscall3" => func!(self::syscalls::___syscall3, [i32, i32] -> [i32]),
            "___syscall4" => func!(self::syscalls::___syscall4, [i32, i32] -> [i32]),
            "___syscall5" => func!(self::syscalls::___syscall5, [i32, i32] -> [i32]),
            "___syscall6" => func!(self::syscalls::___syscall6, [i32, i32] -> [i32]),
            "___syscall12" => func!(self::syscalls::___syscall12, [i32, i32] -> [i32]),
            "___syscall20" => func!(self::syscalls::___syscall20, [] -> [i32]),
            "___syscall39" => func!(self::syscalls::___syscall39, [i32, i32] -> [i32]),
            "___syscall40" => func!(self::syscalls::___syscall40, [i32, i32] -> [i32]),
            "___syscall54" => func!(self::syscalls::___syscall54, [i32, i32] -> [i32]),
            "___syscall57" => func!(self::syscalls::___syscall57, [i32, i32] -> [i32]),
            "___syscall63" => func!(self::syscalls::___syscall63, [i32, i32] -> [i32]),
            "___syscall64" => func!(self::syscalls::___syscall64, [] -> [i32]),
            "___syscall102" => func!(self::syscalls::___syscall102, [i32, i32] -> [i32]),
            "___syscall114" => func!(self::syscalls::___syscall114, [i32, i32] -> [i32]),
            "___syscall122" => func!(self::syscalls::___syscall122, [i32, i32] -> [i32]),
            "___syscall140" => func!(self::syscalls::___syscall140, [i32, i32] -> [i32]),
            "___syscall142" => func!(self::syscalls::___syscall142, [i32, i32] -> [i32]),
            "___syscall145" => func!(self::syscalls::___syscall145, [i32, i32] -> [i32]),
            "___syscall146" => func!(self::syscalls::___syscall146, [i32, i32] -> [i32]),
            "___syscall180" => func!(self::syscalls::___syscall180, [i32, i32] -> [i32]),
            "___syscall181" => func!(self::syscalls::___syscall181, [i32, i32] -> [i32]),
            "___syscall192" => func!(self::syscalls::___syscall192, [i32, i32] -> [i32]),
            "___syscall195" => func!(self::syscalls::___syscall195, [i32, i32] -> [i32]),
            "___syscall197" => func!(self::syscalls::___syscall197, [i32, i32] -> [i32]),
            "___syscall201" => func!(self::syscalls::___syscall201, [] -> [i32]),
            "___syscall202" => func!(self::syscalls::___syscall202, [] -> [i32]),
            "___syscall212" => func!(self::syscalls::___syscall212, [i32, i32] -> [i32]),
            "___syscall221" => func!(self::syscalls::___syscall221, [i32, i32] -> [i32]),
            "___syscall330" => func!(self::syscalls::___syscall330, [i32, i32] -> [i32]),
            "___syscall340" => func!(self::syscalls::___syscall340, [i32, i32] -> [i32]),

            "abort" => func!(self::process::em_abort, [i32] -> []),
            "_abort" => func!(self::process::_abort, [] -> []),
            "abortStackOverflow" => func!(self::process::abort_stack_overflow, [] -> []),
            "_llvm_trap" => func!(self::process::_llvm_trap, [] -> []),
            "_fork" => func!(self::process::_fork, [] -> [i32]),
            "_exit" => func!(self::process::_exit, [i32] -> []),
            "_system" => func!(self::process::_system, [] -> [i32]),
            "_popen" => func!(self::process::_popen, [] -> [i32]),

            "_sigemptyset" => func!(self::signal::_sigemptyset, [i32] -> [i32]),
            "_sigaddset" => func!(self::signal::_sigaddset, [i32, i32] -> [i32]),
            "_sigprocmask" => func!(self::signal::_sigprocmask, [] -> [i32]),
            "_sigaction" => func!(self::signal::_sigaction, [i32, i32, i32] -> [i32]),
            "_signal" => func!(self::signal::_signal, [i32] -> [i32]),

            "abortOnCannotGrowMemory" => func!(self::memory::abort_on_cannot_grow_memory, [] -> []),
            "_emscripten_memcpy_big" => func!(self::memory::_emscripten_memcpy_big, [i32, i32, i32] -> [i32]),
            "enlargeMemory" => func!(self::memory::enlarge_memory, [] -> []),
            "getTotalMemory" => func!(self::memory::get_total_memory, [] -> [i32]),
            "___map_file" => func!(self::memory::___map_file, [] -> [i32]),

            "___cxa_allocate_exception" => func!(self::exception::___cxa_allocate_exception, [i32] -> [i32]),
            "___cxa_throw" => func!(self::exception::___cxa_throw, [i32, i32, i32] -> []),

            "nullFunc_ii" => func!(self::nullfunc::nullfunc_ii, [i32] -> []),
            "nullFunc_iii" => func!(self::nullfunc::nullfunc_iii, [i32] -> []),
            "nullFunc_iiii" => func!(self::nullfunc::nullfunc_iiii, [i32] -> []),
            "nullFunc_iiiii" => func!(self::nullfunc::nullfunc_iiiii, [i32] -> []),
            "nullFunc_iiiiii" => func!(self::nullfunc::nullfunc_iiiiii, [i32] -> []),
            "nullFunc_v" => func!(self::nullfunc::nullfunc_v, [i32] -> []),
            "nullFunc_vi" => func!(self::nullfunc::nullfunc_vi, [i32] -> []),
            "nullFunc_vii" => func!(self::nullfunc::nullfunc_vii, [i32] -> []),
            "nullFunc_viii" => func!(self::nullfunc::nullfunc_viii, [i32] -> []),
            "nullFunc_viiii" => func!(self::nullfunc::nullfunc_viiii, [i32] -> []),
            "nullFunc_viiiii" => func!(self::nullfunc::nullfunc_viiiii, [i32] -> []),
            "nullFunc_viiiiii" => func!(self::nullfunc::nullfunc_viiiiii, [i32] -> []),

            "_gettimeofday" => func!(self::time::_gettimeofday, [i32, i32] -> [i32]),
            "_clock_gettime" => func!(self::time::_clock_gettime, [i32, i32] -> [i32]),
            "___clock_gettime" => func!(self::time::___clock_gettime, [i32, i32] -> [i32]),
            "_clock" => func!(self::time::_clock, [] -> [i32]),
            "_difftime" => func!(self::time::_difftime, [i32, i32] -> [i32]),
            "_asctime" => func!(self::time::_asctime, [i32] -> [i32]),
            "_asctime_r" => func!(self::time::_asctime_r, [i32, i32] -> [i32]),
            "_localtime" => func!(self::time::_localtime, [i32] -> [i32]),
            "_time" => func!(self::time::_time, [i32] -> [i32]),
            "_strftime" => func!(self::time::_strftime, [i32, i32, i32, i32] -> [i32]),
            "_localtime_r" => func!(self::time::_localtime_r, [i32, i32] -> [i32]),

            "_getpagesize" => func!(self::env::_getpagesize, [] -> [i32])
            "_sysconf" => func!(self::env::_sysconf, [i32] -> [i32]),
            "_llvm_log10_f64" => func!(self::math::_llvm_log10_f64, [f64] -> [f64]),
            "_llvm_log2_f64" => func!(self::math::_llvm_log2_f64, [f64] -> [f64]),

            "__setjmp" => func!(self::jmp::__setjmp, [i32] -> [i32]),
            "__longjmp" => func!(self::jmp::__longjmp, [i32, i32] -> []),
            
        },
        "asm2wasm" => {
            "f64-rem" => func!(self::math::f64_rem, [f64, f64] -> [f64]),
        },
    };

    mock_external!(env_namespace, _waitpid);
    mock_external!(env_namespace, _utimes);
    mock_external!(env_namespace, _usleep);
    // mock_external!(env_namespace, _time);
    // mock_external!(env_namespace, _sysconf);
    // mock_external!(env_namespace, _strftime);
    mock_external!(env_namespace, _sigsuspend);
    // mock_external!(env_namespace, _sigprocmask);
    // mock_external!(env_namespace, _sigemptyset);
    // mock_external!(env_namespace, _sigaddset);
    // mock_external!(env_namespace, _sigaction);
    mock_external!(env_namespace, _setitimer);
    mock_external!(env_namespace, _setgroups);
    mock_external!(env_namespace, _setgrent);
    mock_external!(env_namespace, _sem_wait);
    mock_external!(env_namespace, _sem_post);
    mock_external!(env_namespace, _sem_init);
    mock_external!(env_namespace, _sched_yield);
    mock_external!(env_namespace, _raise);
    mock_external!(env_namespace, _mktime);
    // mock_external!(env_namespace, _localtime_r);
    // mock_external!(env_namespace, _localtime);
    mock_external!(env_namespace, _llvm_stacksave);
    mock_external!(env_namespace, _llvm_stackrestore);
    mock_external!(env_namespace, _kill);
    mock_external!(env_namespace, _gmtime_r);
    // mock_external!(env_namespace, _gettimeofday);
    // mock_external!(env_namespace, _getpagesize);
    mock_external!(env_namespace, _getgrent);
    mock_external!(env_namespace, _getaddrinfo);
    // mock_external!(env_namespace, _fork);
    // mock_external!(env_namespace, _exit);
    mock_external!(env_namespace, _execve);
    mock_external!(env_namespace, _endgrent);
    // mock_external!(env_namespace, _clock_gettime);
    mock_external!(env_namespace, ___syscall97);
    mock_external!(env_namespace, ___syscall91);
    mock_external!(env_namespace, ___syscall85);
    mock_external!(env_namespace, ___syscall75);
    mock_external!(env_namespace, ___syscall66);
    // mock_external!(env_namespace, ___syscall64);
    // mock_external!(env_namespace, ___syscall63);
    // mock_external!(env_namespace, ___syscall60);
    // mock_external!(env_namespace, ___syscall54);
    // mock_external!(env_namespace, ___syscall39);
    mock_external!(env_namespace, ___syscall38);
    // mock_external!(env_namespace, ___syscall340);
    mock_external!(env_namespace, ___syscall334);
    mock_external!(env_namespace, ___syscall300);
    mock_external!(env_namespace, ___syscall295);
    mock_external!(env_namespace, ___syscall272);
    mock_external!(env_namespace, ___syscall268);
    // mock_external!(env_namespace, ___syscall221);
    mock_external!(env_namespace, ___syscall220);
    // mock_external!(env_namespace, ___syscall212);
    // mock_external!(env_namespace, ___syscall201);
    mock_external!(env_namespace, ___syscall199);
    // mock_external!(env_namespace, ___syscall197);
    mock_external!(env_namespace, ___syscall196);
    // mock_external!(env_namespace, ___syscall195);
    mock_external!(env_namespace, ___syscall194);
    mock_external!(env_namespace, ___syscall191);
    // mock_external!(env_namespace, ___syscall181);
    // mock_external!(env_namespace, ___syscall180);
    mock_external!(env_namespace, ___syscall168);
    // mock_external!(env_namespace, ___syscall146);
    // mock_external!(env_namespace, ___syscall145);
    // mock_external!(env_namespace, ___syscall142);
    mock_external!(env_namespace, ___syscall140);
    // mock_external!(env_namespace, ___syscall122);
    // mock_external!(env_namespace, ___syscall102);
    // mock_external!(env_namespace, ___syscall20);
    mock_external!(env_namespace, ___syscall15);
    mock_external!(env_namespace, ___syscall10);
    mock_external!(env_namespace, _dlopen);
    mock_external!(env_namespace, _dlclose);
    mock_external!(env_namespace, _dlsym);
    mock_external!(env_namespace, _dlerror);

    imports.register("env", env_namespace);
    imports.register("asm2wasm", asm_namespace);

    imports
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
