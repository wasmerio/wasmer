#[macro_use]
extern crate wasmer_runtime;

use byteorder::{ByteOrder, LittleEndian};
use hashbrown::HashMap;
use std::mem;
use wasmer_runtime::{
    export::{Context, Export, FuncPointer, GlobalPointer},
    import::{Imports, NamespaceMap},
    memory::LinearMemory,
    types::{
        FuncSig, GlobalDesc,
        Type::{self, *},
    },
    vm::LocalGlobal,
};

//#[cfg(test)]
mod file_descriptor;
pub mod stdio;

// EMSCRIPTEN APIS
#[macro_use]
mod macros;
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

pub fn emscripten_set_up_memory(memory: &mut LinearMemory) {
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

pub fn generate_emscripten_env(globals: &EmscriptenGlobals) -> Imports {
    let mut imports = Imports::new();
    let mut env_namespace = NamespaceMap::new();
    let mut asm_namespace = NamespaceMap::new();
    let mut global_namespace = NamespaceMap::new();

    // Add globals.
    // NOTE: There is really no need for checks, these globals should always be available.
    let env_globals = globals.data.get("env").unwrap();
    let global_globals = globals.data.get("global").unwrap();

    let (value, ty) = env_globals.get("STACKTOP").unwrap();
    env_namespace.insert(
        "STACKTOP".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    let (value, ty) = env_globals.get("STACK_MAX").unwrap();
    env_namespace.insert(
        "STACK_MAX".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    let (value, ty) = env_globals.get("DYNAMICTOP_PTR").unwrap();
    env_namespace.insert(
        "DYNAMICTOP_PTR".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    let (value, ty) = env_globals.get("tableBase").unwrap();
    env_namespace.insert(
        "tableBase".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    let (value, ty) = global_globals.get("Infinity").unwrap();
    global_namespace.insert(
        "Infinity".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    let (value, ty) = global_globals.get("NaN").unwrap();
    global_namespace.insert(
        "NaN".to_string(),
        Export::Global {
            local: global!(value),
            global: GlobalDesc {
                mutable: false,
                ty: ty.clone(),
            },
        },
    );

    // Print function
    env_namespace.insert(
        "printf",
        Export::Function {
            func: func!(io, printf),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "putchar",
        Export::Function {
            func: func!(io, putchar),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    // Lock
    env_namespace.insert(
        "___lock",
        Export::Function {
            func: func!(lock, ___lock),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "___unlock",
        Export::Function {
            func: func!(lock, ___unlock),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "___wait",
        Export::Function {
            func: func!(lock, ___wait),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );
    // Env
    env_namespace.insert(
        "_getenv",
        Export::Function {
            func: func!(env, _getenv),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_setenv",
        Export::Function {
            func: func!(env, _setenv),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_putenv",
        Export::Function {
            func: func!(env, _putenv),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_unsetenv",
        Export::Function {
            func: func!(env, _unsetenv),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_getpwnam",
        Export::Function {
            func: func!(env, _getpwnam),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_getgrnam",
        Export::Function {
            func: func!(env, _getgrnam),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___buildEnvironment",
        Export::Function {
            func: func!(env, ___build_environment),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Errno
    env_namespace.insert(
        "___setErrNo",
        Export::Function {
            func: func!(errno, ___seterrno),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Syscalls
    env_namespace.insert(
        "___syscall1",
        Export::Function {
            func: func!(syscalls, ___syscall1),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "___syscall3",
        Export::Function {
            func: func!(syscalls, ___syscall3),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall4",
        Export::Function {
            func: func!(syscalls, ___syscall4),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall5",
        Export::Function {
            func: func!(syscalls, ___syscall5),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall6",
        Export::Function {
            func: func!(syscalls, ___syscall6),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall12",
        Export::Function {
            func: func!(syscalls, ___syscall12),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall20",
        Export::Function {
            func: func!(syscalls, ___syscall20),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall39",
        Export::Function {
            func: func!(syscalls, ___syscall39),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall40",
        Export::Function {
            func: func!(syscalls, ___syscall40),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall54",
        Export::Function {
            func: func!(syscalls, ___syscall54),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall57",
        Export::Function {
            func: func!(syscalls, ___syscall57),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall63",
        Export::Function {
            func: func!(syscalls, ___syscall63),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall64",
        Export::Function {
            func: func!(syscalls, ___syscall64),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall102",
        Export::Function {
            func: func!(syscalls, ___syscall102),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall114",
        Export::Function {
            func: func!(syscalls, ___syscall114),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall122",
        Export::Function {
            func: func!(syscalls, ___syscall122),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall140",
        Export::Function {
            func: func!(syscalls, ___syscall140),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall142",
        Export::Function {
            func: func!(syscalls, ___syscall142),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall145",
        Export::Function {
            func: func!(syscalls, ___syscall145),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall146",
        Export::Function {
            func: func!(syscalls, ___syscall146),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall180",
        Export::Function {
            func: func!(syscalls, ___syscall180),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall181",
        Export::Function {
            func: func!(syscalls, ___syscall181),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall192",
        Export::Function {
            func: func!(syscalls, ___syscall192),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall195",
        Export::Function {
            func: func!(syscalls, ___syscall195),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall197",
        Export::Function {
            func: func!(syscalls, ___syscall197),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall201",
        Export::Function {
            func: func!(syscalls, ___syscall201),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall202",
        Export::Function {
            func: func!(syscalls, ___syscall202),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall212",
        Export::Function {
            func: func!(syscalls, ___syscall212),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall221",
        Export::Function {
            func: func!(syscalls, ___syscall221),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall330",
        Export::Function {
            func: func!(syscalls, ___syscall330),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___syscall340",
        Export::Function {
            func: func!(syscalls, ___syscall340),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    // Process
    env_namespace.insert(
        "abort",
        Export::Function {
            func: func!(process, em_abort),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_abort",
        Export::Function {
            func: func!(process, _abort),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "abortStackOverflow",
        Export::Function {
            func: func!(process, abort_stack_overflow),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_llvm_trap",
        Export::Function {
            func: func!(process, _llvm_trap),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_fork",
        Export::Function {
            func: func!(process, _fork),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_exit",
        Export::Function {
            func: func!(process, _exit),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "_system",
        Export::Function {
            func: func!(process, _system),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_popen",
        Export::Function {
            func: func!(process, _popen),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    // Signal
    env_namespace.insert(
        "_sigemptyset",
        Export::Function {
            func: func!(signal, _sigemptyset),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_sigaddset",
        Export::Function {
            func: func!(signal, _sigaddset),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_sigprocmask",
        Export::Function {
            func: func!(signal, _sigprocmask),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_sigaction",
        Export::Function {
            func: func!(signal, _sigaction),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_signal",
        Export::Function {
            func: func!(signal, _signal),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    // Memory
    env_namespace.insert(
        "abortOnCannotGrowMemory",
        Export::Function {
            func: func!(memory, abort_on_cannot_grow_memory),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_emscripten_memcpy_big",
        Export::Function {
            func: func!(memory, _emscripten_memcpy_big),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "enlargeMemory",
        Export::Function {
            func: func!(memory, enlarge_memory),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "getTotalMemory",
        Export::Function {
            func: func!(memory, get_total_memory),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___map_file",
        Export::Function {
            func: func!(memory, ___map_file),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    // Exception
    env_namespace.insert(
        "___cxa_allocate_exception",
        Export::Function {
            func: func!(exception, ___cxa_allocate_exception),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___cxa_allocate_exception",
        Export::Function {
            func: func!(exception, ___cxa_throw),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "___cxa_throw",
        Export::Function {
            func: func!(exception, ___cxa_throw),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );
    // NullFuncs
    env_namespace.insert(
        "nullFunc_ii",
        Export::Function {
            func: func!(nullfunc, nullfunc_ii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_iii",
        Export::Function {
            func: func!(nullfunc, nullfunc_iii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_iiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_iiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_iiiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_iiiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_iiiiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_iiiiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_v",
        Export::Function {
            func: func!(nullfunc, nullfunc_v),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_vi",
        Export::Function {
            func: func!(nullfunc, nullfunc_vi),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_vii",
        Export::Function {
            func: func!(nullfunc, nullfunc_vii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_viii",
        Export::Function {
            func: func!(nullfunc, nullfunc_viii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_viiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_viiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_viiiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_viiiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );

    env_namespace.insert(
        "nullFunc_viiiiii",
        Export::Function {
            func: func!(nullfunc, nullfunc_viiiiii),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Time
    env_namespace.insert(
        "_gettimeofday",
        Export::Function {
            func: func!(time, _gettimeofday),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_clock_gettime",
        Export::Function {
            func: func!(time, _clock_gettime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "___clock_gettime",
        Export::Function {
            func: func!(time, ___clock_gettime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_clock",
        Export::Function {
            func: func!(time, _clock),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_difftime",
        Export::Function {
            func: func!(time, _difftime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_asctime",
        Export::Function {
            func: func!(time, _asctime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_asctime_r",
        Export::Function {
            func: func!(time, _asctime_r),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_localtime",
        Export::Function {
            func: func!(time, _localtime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_time",
        Export::Function {
            func: func!(time, _time),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_strftime",
        Export::Function {
            func: func!(time, _strftime),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_localtime_r",
        Export::Function {
            func: func!(time, _localtime_r),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_getpagesize",
        Export::Function {
            func: func!(env, _getpagesize),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "_sysconf",
        Export::Function {
            func: func!(env, _sysconf),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    // Math
    asm_namespace.insert(
        "f64-rem",
        Export::Function {
            func: func!(math, f64_rem),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64, F64],
                returns: vec![F64],
            },
        },
    );

    env_namespace.insert(
        "_llvm_log10_f64",
        Export::Function {
            func: func!(math, _llvm_log10_f64),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64],
                returns: vec![F64],
            },
        },
    );

    env_namespace.insert(
        "_llvm_log2_f64",
        Export::Function {
            func: func!(math, _llvm_log2_f64),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64],
                returns: vec![F64],
            },
        },
    );

    //
    env_namespace.insert(
        "__setjmp",
        Export::Function {
            func: func!(jmp, __setjmp),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );

    env_namespace.insert(
        "__longjmp",
        Export::Function {
            func: func!(jmp, __longjmp),
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );

    // mock_external!(env_namespace, _waitpid);
    // mock_external!(env_namespace, _utimes);
    // mock_external!(env_namespace, _usleep);
    // // mock_external!(env_namespace, _time);
    // // mock_external!(env_namespace, _sysconf);
    // // mock_external!(env_namespace, _strftime);
    // mock_external!(env_namespace, _sigsuspend);
    // // mock_external!(env_namespace, _sigprocmask);
    // // mock_external!(env_namespace, _sigemptyset);
    // // mock_external!(env_namespace, _sigaddset);
    // // mock_external!(env_namespace, _sigaction);
    // mock_external!(env_namespace, _setitimer);
    // mock_external!(env_namespace, _setgroups);
    // mock_external!(env_namespace, _setgrent);
    // mock_external!(env_namespace, _sem_wait);
    // mock_external!(env_namespace, _sem_post);
    // mock_external!(env_namespace, _sem_init);
    // mock_external!(env_namespace, _sched_yield);
    // mock_external!(env_namespace, _raise);
    // mock_external!(env_namespace, _mktime);
    // // mock_external!(env_namespace, _localtime_r);
    // // mock_external!(env_namespace, _localtime);
    // mock_external!(env_namespace, _llvm_stacksave);
    // mock_external!(env_namespace, _llvm_stackrestore);
    // mock_external!(env_namespace, _kill);
    // mock_external!(env_namespace, _gmtime_r);
    // // mock_external!(env_namespace, _gettimeofday);
    // // mock_external!(env_namespace, _getpagesize);
    // mock_external!(env_namespace, _getgrent);
    // mock_external!(env_namespace, _getaddrinfo);
    // // mock_external!(env_namespace, _fork);
    // // mock_external!(env_namespace, _exit);
    // mock_external!(env_namespace, _execve);
    // mock_external!(env_namespace, _endgrent);
    // // mock_external!(env_namespace, _clock_gettime);
    // mock_external!(env_namespace, ___syscall97);
    // mock_external!(env_namespace, ___syscall91);
    // mock_external!(env_namespace, ___syscall85);
    // mock_external!(env_namespace, ___syscall75);
    // mock_external!(env_namespace, ___syscall66);
    // // mock_external!(env_namespace, ___syscall64);
    // // mock_external!(env_namespace, ___syscall63);
    // // mock_external!(env_namespace, ___syscall60);
    // // mock_external!(env_namespace, ___syscall54);
    // // mock_external!(env_namespace, ___syscall39);
    // mock_external!(env_namespace, ___syscall38);
    // // mock_external!(env_namespace, ___syscall340);
    // mock_external!(env_namespace, ___syscall334);
    // mock_external!(env_namespace, ___syscall300);
    // mock_external!(env_namespace, ___syscall295);
    // mock_external!(env_namespace, ___syscall272);
    // mock_external!(env_namespace, ___syscall268);
    // // mock_external!(env_namespace, ___syscall221);
    // mock_external!(env_namespace, ___syscall220);
    // // mock_external!(env_namespace, ___syscall212);
    // // mock_external!(env_namespace, ___syscall201);
    // mock_external!(env_namespace, ___syscall199);
    // // mock_external!(env_namespace, ___syscall197);
    // mock_external!(env_namespace, ___syscall196);
    // // mock_external!(env_namespace, ___syscall195);
    // mock_external!(env_namespace, ___syscall194);
    // mock_external!(env_namespace, ___syscall191);
    // // mock_external!(env_namespace, ___syscall181);
    // // mock_external!(env_namespace, ___syscall180);
    // mock_external!(env_namespace, ___syscall168);
    // // mock_external!(env_namespace, ___syscall146);
    // // mock_external!(env_namespace, ___syscall145);
    // // mock_external!(env_namespace, ___syscall142);
    // mock_external!(env_namespace, ___syscall140);
    // // mock_external!(env_namespace, ___syscall122);
    // // mock_external!(env_namespace, ___syscall102);
    // // mock_external!(env_namespace, ___syscall20);
    // mock_external!(env_namespace, ___syscall15);
    mock_external!(env_namespace, ___syscall10, [ I32, I32 => I32 ]);
    // mock_external!(env_namespace, _dlopen);
    // mock_external!(env_namespace, _dlclose);
    // mock_external!(env_namespace, _dlsym);
    // mock_external!(env_namespace, _dlerror);

    imports.register("env", env_namespace);
    imports.register("asm2wasm", asm_namespace);

    imports
}
