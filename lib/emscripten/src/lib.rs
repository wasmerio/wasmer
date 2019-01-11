#[macro_use]
extern crate wasmer_runtime;

use wasmer_runtime::{
    instance::{FuncRef},
    import::{Imports},
    export::{Export, Context},
    types::{
        FuncSig, Type::*, Value,
        GlobalDesc,
    },
    vm::{self, LocalGlobal},
    memory::LinearMemory,
};
use byteorder::{ByteOrder, LittleEndian};
use libc::c_int;
use std::cell::UnsafeCell;
use std::mem;
use hashbrown::{hash_map::Entry, HashMap};

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

macro_rules! mock_external {
    ($imports:ident, $name:ident) => {{
        extern "C" fn _mocked_fn() -> i32 {
            debug!("emscripten::{} <mock>", stringify!($name));
            -1
        }

        $imports.register_export(
            "env",
            stringify!($name),
            Export::Function {
                func: unsafe { FuncRef::new(_mocked_fn as _) },
                ctx: Context::Internal,
                signature: FuncSig {
                    params: vec![],
                    returns: vec![I32],
                },
            },
        );
    }};
}

pub struct EmscriptenGlobals {
    pub data: Vec<(String, LocalGlobal, GlobalDesc)>,
}

impl EmscriptenGlobals {
    pub fn new() -> Self {
        let mut data = Vec::new();

        data.push((
            "STACKTOP".into(),
            LocalGlobal { data: stacktop(STATIC_BUMP) as _ },
            GlobalDesc { mutable: false, ty: I32 }),
        );

        data.push((
            "DYNAMICTOP_PTR".into(),
            LocalGlobal { data: dynamictop_ptr(STATIC_BUMP) as _ },
            GlobalDesc { mutable: false, ty: I32 }),
        );

        data.push((
            "Infinity".into(),
            LocalGlobal { data: std::f64::INFINITY.to_bits() },
            GlobalDesc { mutable: false, ty: F64 },
        ));

        data.push((
            "NaN".into(),
            LocalGlobal { data: std::f64::NAN.to_bits() },
            GlobalDesc { mutable: false, ty: F64 },
        ));

        data.push((
            "tableBase".into(),
            LocalGlobal { data: 0 },
            GlobalDesc { mutable: false, ty: I32 },
        ));

        Self {
            data,
        }
    }
}

pub fn generate_emscripten_env(globals: &EmscriptenGlobals) -> Imports {
    let mut imports = Imports::new();

    // Add globals.
    for (name, global, desc) in &globals.data {
        let export = Export::Global {
            local: unsafe { std::mem::transmute::<&LocalGlobal, *mut LocalGlobal>(global) },
            global: desc.clone(),
        };

        imports.register_export("env", name.clone(), export);
    }

    // Print functions
    imports.register_export(
        "env",
        "printf",
        Export::Function {
            func: unsafe { FuncRef::new(io::printf as *const _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            }
        },
    );

    imports.register_export(
        "env",
        "printf",
        Export::Function {
            func: unsafe { FuncRef::new(io::printf as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );

    imports.register_export(
        "env",
        "putchar",
        Export::Function {
            func: unsafe { FuncRef::new(io::putchar as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Lock
    imports.register_export(
        "env",
        "___lock",
        Export::Function {
            func: unsafe { FuncRef::new(lock::___lock as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "___unlock",
        Export::Function {
            func: unsafe { FuncRef::new(lock::___unlock as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "___wait",
        Export::Function {
            func: unsafe { FuncRef::new(lock::___wait as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );
    // Env
    imports.register_export(
        "env",
        "_getenv",
        Export::Function {
            func: unsafe { FuncRef::new(env::_getenv as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_setenv",
        Export::Function {
            func: unsafe { FuncRef::new(env::_setenv as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_putenv",
        Export::Function {
            func: unsafe { FuncRef::new(env::_putenv as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_unsetenv",
        Export::Function {
            func: unsafe { FuncRef::new(env::_unsetenv as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_getpwnam",
        Export::Function {
            func: unsafe { FuncRef::new(env::_getpwnam as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_getgrnam",
        Export::Function {
            func: unsafe { FuncRef::new(env::_getgrnam as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___buildEnvironment",
        Export::Function {
            func: unsafe { FuncRef::new(env::___build_environment as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Errno
    imports.register_export(
        "env",
        "___setErrNo",
        Export::Function {
            func: unsafe { FuncRef::new(errno::___seterrno as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    // Syscalls
    imports.register_export(
        "env",
        "___syscall1",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall1 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall3",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall3 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall4",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall4 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall5",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall5 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall6",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall6 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall12",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall12 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall20",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall20 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall39",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall39 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall40",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall40 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall54",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall54 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall57",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall57 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall63",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall63 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall64",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall64 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall102",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall102 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall114",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall114 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall122",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall122 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall140",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall140 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall142",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall142 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall145",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall145 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall146",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall146 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall180",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall180 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall181",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall181 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall192",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall192 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall195",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall195 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall197",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall197 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall201",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall201 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall202",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall202 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall212",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall212 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall221",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall221 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall330",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall330 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___syscall340",
        Export::Function {
            func: unsafe { FuncRef::new(syscalls::___syscall340 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    // Process
    imports.register_export(
        "env",
        "abort",
        Export::Function {
            func: unsafe { FuncRef::new(process::em_abort as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_abort",
        Export::Function {
            func: unsafe { FuncRef::new(process::_abort as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "abortStackOverflow",
        Export::Function {
            func: unsafe { FuncRef::new(process::abort_stack_overflow as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_llvm_trap",
        Export::Function {
            func: unsafe { FuncRef::new(process::_llvm_trap as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_fork",
        Export::Function {
            func: unsafe { FuncRef::new(process::_fork as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_exit",
        Export::Function {
            func: unsafe { FuncRef::new(process::_exit as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_system",
        Export::Function {
            func: unsafe { FuncRef::new(process::_system as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_popen",
        Export::Function {
            func: unsafe { FuncRef::new(process::_popen as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    // Signal
    imports.register_export(
        "env",
        "_sigemptyset",
        Export::Function {
            func: unsafe { FuncRef::new(signal::_sigemptyset as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_sigaddset",
        Export::Function {
            func: unsafe { FuncRef::new(signal::_sigaddset as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_sigprocmask",
        Export::Function {
            func: unsafe { FuncRef::new(signal::_sigprocmask as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_sigaction",
        Export::Function {
            func: unsafe { FuncRef::new(signal::_sigaction as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_signal",
        Export::Function {
            func: unsafe { FuncRef::new(signal::_signal as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    // Memory
    imports.register_export(
        "env",
        "abortOnCannotGrowMemory",
        Export::Function {
            func: unsafe { FuncRef::new(memory::abort_on_cannot_grow_memory as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "_emscripten_memcpy_big",
        Export::Function {
            func: unsafe { FuncRef::new(memory::_emscripten_memcpy_big as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "enlargeMemory",
        Export::Function {
            func: unsafe { FuncRef::new(memory::enlarge_memory as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "getTotalMemory",
        Export::Function {
            func: unsafe { FuncRef::new(memory::get_total_memory as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___map_file",
        Export::Function {
            func: unsafe { FuncRef::new(memory::___map_file as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    // Exception
    imports.register_export(
        "env",
        "___cxa_allocate_exception",
        Export::Function {
            func: unsafe { FuncRef::new(exception::___cxa_allocate_exception as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___cxa_allocate_exception",
        Export::Function {
            func: unsafe { FuncRef::new(exception::___cxa_throw as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "___cxa_throw",
        Export::Function {
            func: unsafe { FuncRef::new(exception::___cxa_throw as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32],
                returns: vec![],
            },
        },
    );
    // NullFuncs
    imports.register_export(
        "env",
        "nullFunc_ii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_ii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_iii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_iii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_iiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_iiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_iiiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_iiiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_iiiiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_iiiiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_v",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_v as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_vi",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_vi as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_vii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_vii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_viii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_viii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_viiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_viiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_viiiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_viiiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    imports.register_export(
        "env",
        "nullFunc_viiiiii",
        Export::Function {
            func: unsafe { FuncRef::new(nullfunc::nullfunc_viiiiii as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![],
            },
        },
    );
    // Time
    imports.register_export(
        "env",
        "_gettimeofday",
        Export::Function {
            func: unsafe { FuncRef::new(time::_gettimeofday as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_clock_gettime",
        Export::Function {
            func: unsafe { FuncRef::new(time::_clock_gettime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "___clock_gettime",
        Export::Function {
            func: unsafe { FuncRef::new(time::___clock_gettime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_clock",
        Export::Function {
            func: unsafe { FuncRef::new(time::_clock as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_difftime",
        Export::Function {
            func: unsafe { FuncRef::new(time::_difftime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_asctime",
        Export::Function {
            func: unsafe { FuncRef::new(time::_asctime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_asctime_r",
        Export::Function {
            func: unsafe { FuncRef::new(time::_asctime_r as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_localtime",
        Export::Function {
            func: unsafe { FuncRef::new(time::_localtime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_time",
        Export::Function {
            func: unsafe { FuncRef::new(time::_time as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_strftime",
        Export::Function {
            func: unsafe { FuncRef::new(time::_strftime as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32, I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_localtime_r",
        Export::Function {
            func: unsafe { FuncRef::new(time::_localtime_r as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_getpagesize",
        Export::Function {
            func: unsafe { FuncRef::new(env::_getpagesize as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "_sysconf",
        Export::Function {
            func: unsafe { FuncRef::new(env::_sysconf as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    // Math
    imports.register_export(
        "env",
        "_llvm_log10_f64",
        Export::Function {
            func: unsafe { FuncRef::new(math::_llvm_log10_f64 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64],
                returns: vec![F64],
            },
        },
    );
    imports.register_export(
        "env",
        "_llvm_log2_f64",
        Export::Function {
            func: unsafe { FuncRef::new( math::_llvm_log2_f64 as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64],
                returns: vec![F64],
            },
        },
    );
    imports.register_export(
        "asm2wasm",
        "f64-rem",
        Export::Function {
            func: unsafe { FuncRef::new(math::f64_rem as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![F64, F64],
                returns: vec![F64],
            },
        },
    );
    //
    imports.register_export(
        "env",
        "__setjmp",
        Export::Function {
            func: unsafe { FuncRef::new(jmp::__setjmp as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32],
                returns: vec![I32],
            },
        },
    );
    imports.register_export(
        "env",
        "__longjmp",
        Export::Function {
            func: unsafe { FuncRef::new(jmp::__longjmp as _) },
            ctx: Context::Internal,
            signature: FuncSig {
                params: vec![I32, I32],
                returns: vec![],
            },
        },
    );

    mock_external!(imports, _waitpid);
    mock_external!(imports, _utimes);
    mock_external!(imports, _usleep);
    // mock_external!(imports, _time);
    // mock_external!(imports, _sysconf);
    // mock_external!(imports, _strftime);
    mock_external!(imports, _sigsuspend);
    // mock_external!(imports, _sigprocmask);
    // mock_external!(imports, _sigemptyset);
    // mock_external!(imports, _sigaddset);
    // mock_external!(imports, _sigaction);
    mock_external!(imports, _setitimer);
    mock_external!(imports, _setgroups);
    mock_external!(imports, _setgrent);
    mock_external!(imports, _sem_wait);
    mock_external!(imports, _sem_post);
    mock_external!(imports, _sem_init);
    mock_external!(imports, _sched_yield);
    mock_external!(imports, _raise);
    mock_external!(imports, _mktime);
    // mock_external!(imports, _localtime_r);
    // mock_external!(imports, _localtime);
    mock_external!(imports, _llvm_stacksave);
    mock_external!(imports, _llvm_stackrestore);
    mock_external!(imports, _kill);
    mock_external!(imports, _gmtime_r);
    // mock_external!(imports, _gettimeofday);
    // mock_external!(imports, _getpagesize);
    mock_external!(imports, _getgrent);
    mock_external!(imports, _getaddrinfo);
    // mock_external!(imports, _fork);
    // mock_external!(imports, _exit);
    mock_external!(imports, _execve);
    mock_external!(imports, _endgrent);
    // mock_external!(imports, _clock_gettime);
    mock_external!(imports, ___syscall97);
    mock_external!(imports, ___syscall91);
    mock_external!(imports, ___syscall85);
    mock_external!(imports, ___syscall75);
    mock_external!(imports, ___syscall66);
    // mock_external!(imports, ___syscall64);
    // mock_external!(imports, ___syscall63);
    // mock_external!(imports, ___syscall60);
    // mock_external!(imports, ___syscall54);
    // mock_external!(imports, ___syscall39);
    mock_external!(imports, ___syscall38);
    // mock_external!(imports, ___syscall340);
    mock_external!(imports, ___syscall334);
    mock_external!(imports, ___syscall300);
    mock_external!(imports, ___syscall295);
    mock_external!(imports, ___syscall272);
    mock_external!(imports, ___syscall268);
    // mock_external!(imports, ___syscall221);
    mock_external!(imports, ___syscall220);
    // mock_external!(imports, ___syscall212);
    // mock_external!(imports, ___syscall201);
    mock_external!(imports, ___syscall199);
    // mock_external!(imports, ___syscall197);
    mock_external!(imports, ___syscall196);
    // mock_external!(imports, ___syscall195);
    mock_external!(imports, ___syscall194);
    mock_external!(imports, ___syscall191);
    // mock_external!(imports, ___syscall181);
    // mock_external!(imports, ___syscall180);
    mock_external!(imports, ___syscall168);
    // mock_external!(imports, ___syscall146);
    // mock_external!(imports, ___syscall145);
    // mock_external!(imports, ___syscall142);
    mock_external!(imports, ___syscall140);
    // mock_external!(imports, ___syscall122);
    // mock_external!(imports, ___syscall102);
    // mock_external!(imports, ___syscall20);
    mock_external!(imports, ___syscall15);
    mock_external!(imports, ___syscall10);
    mock_external!(imports, _dlopen);
    mock_external!(imports, _dlclose);
    mock_external!(imports, _dlsym);
    mock_external!(imports, _dlerror);

    imports
}
