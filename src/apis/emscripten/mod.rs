use byteorder::{ByteOrder, LittleEndian};
/// NOTE: TODO: These emscripten api implementation only support wasm32 for now because they assume offsets are u32
use crate::webassembly::{ImportObject, ImportValue, LinearMemory};
use std::mem;

// EMSCRIPTEN APIS
mod env;
mod errno;
mod io;
mod lock;
mod memory;
mod nullfunc;
mod process;
mod signal;
mod storage;
mod syscalls;
mod time;
mod utils;
mod varargs;

pub use self::storage::{align_memory, static_alloc};
pub use self::utils::is_emscripten_module;

// TODO: Magic number - how is this calculated?
const TOTAL_STACK: u32 = 5242880;
// TODO: Magic number stolen from the generated JS - how is this calculated?
const DYNAMICTOP_PTR_DIFF: u32 = 1088;

const STATIC_BUMP: u32 = 215536; // TODO: make this variable

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

// fn static_alloc(size: usize, static_top: &mut size) -> usize {
//     let ret = *static_top;
//     *static_top = (*static_top + size + 15) & (-16 as usize);
//     ret
// }

pub fn emscripten_set_up_memory(memory: &mut LinearMemory) {
    let dynamictop_ptr = dynamictop_ptr(STATIC_BUMP) as usize;
    let dynamictop_ptr_offset = dynamictop_ptr + mem::size_of::<u32>();

    // We avoid failures of setting the u32 in our memory if it's out of bounds
    if dynamictop_ptr_offset > memory.len() {
        return;
    }
    let mem = &mut memory[dynamictop_ptr..dynamictop_ptr_offset];
    LittleEndian::write_u32(mem, dynamic_base(STATIC_BUMP));
}

macro_rules! mock_external {
    ($import:ident, $name:ident) => {{
        extern "C" fn _mocked_fn() -> i32 {
            debug!("emscripten::{} <mock>", stringify!($name));
            -1
        }
        $import.set("env", stringify!($name), ImportValue::Func(_mocked_fn as _));
    }};
}

pub fn generate_emscripten_env<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    // Global
    import_object.set(
        "env",
        "global1",
        ImportValue::Global(24), // TODO
    );
    import_object.set(
        "env",
        "global2",
        ImportValue::Global(50), // TODO
    );
    import_object.set(
        "env",
        "global3",
        ImportValue::Global(67), // TODO
    );

    import_object.set(
        "env",
        "STACKTOP",
        ImportValue::Global(stacktop(STATIC_BUMP) as _),
    );
    import_object.set(
        "env",
        "STACK_MAX",
        ImportValue::Global(stack_max(STATIC_BUMP) as _),
    );
    import_object.set(
        "env",
        "DYNAMICTOP_PTR",
        ImportValue::Global(dynamictop_ptr(STATIC_BUMP) as _),
    );
    import_object.set("env", "tableBase", ImportValue::Global(0));

    // Print functions
    import_object.set("env", "printf", ImportValue::Func(io::printf as _));
    import_object.set("env", "putchar", ImportValue::Func(io::putchar as _));
    // Lock
    import_object.set("env", "___lock", ImportValue::Func(lock::___lock as _));
    import_object.set("env", "___unlock", ImportValue::Func(lock::___unlock as _));
    // Env
    import_object.set("env", "_getenv", ImportValue::Func(env::_getenv as _));
    import_object.set("env", "_getpwnam", ImportValue::Func(env::_getpwnam as _));
    import_object.set("env", "_getgrnam", ImportValue::Func(env::_getgrnam as _));
    import_object.set("env", "___buildEnvironment", ImportValue::Func(env::___build_environment as _));
    // Errno
    import_object.set(
        "env",
        "___setErrNo",
        ImportValue::Func(errno::___seterrno as _),
    );
    // Syscalls
    import_object.set(
        "env",
        "___syscall1",
        ImportValue::Func(syscalls::___syscall1 as _),
    );
    import_object.set(
        "env",
        "___syscall3",
        ImportValue::Func(syscalls::___syscall3 as _),
    );
    import_object.set(
        "env",
        "___syscall4",
        ImportValue::Func(syscalls::___syscall4 as _),
    );
    import_object.set(
        "env",
        "___syscall5",
        ImportValue::Func(syscalls::___syscall5 as _),
    );
    import_object.set(
        "env",
        "___syscall6",
        ImportValue::Func(syscalls::___syscall6 as _),
    );
    import_object.set(
        "env",
        "___syscall54",
        ImportValue::Func(syscalls::___syscall54 as _),
    );
    import_object.set(
        "env",
        "___syscall140",
        ImportValue::Func(syscalls::___syscall140 as _),
    );
    import_object.set(
        "env",
        "___syscall145",
        ImportValue::Func(syscalls::___syscall145 as _),
    );
    import_object.set(
        "env",
        "___syscall146",
        ImportValue::Func(syscalls::___syscall146 as _),
    );
    import_object.set(
        "env",
        "___syscall221",
        ImportValue::Func(syscalls::___syscall221 as _),
    );
    import_object.set(
        "env",
        "___syscall20",
        ImportValue::Func(syscalls::___syscall20 as _),
    );
    import_object.set(
        "env",
        "___syscall64",
        ImportValue::Func(syscalls::___syscall64 as _),
    );
    import_object.set(
        "env",
        "___syscall122",
        ImportValue::Func(syscalls::___syscall122 as _),
    );
    import_object.set(
        "env",
        "___syscall201",
        ImportValue::Func(syscalls::___syscall201 as _),
    );
    import_object.set(
        "env",
        "___syscall202",
        ImportValue::Func(syscalls::___syscall202 as _),
    );
    import_object.set(
        "env",
        "___syscall340",
        ImportValue::Func(syscalls::___syscall340 as _),
    );
    import_object.set(
        "env",
        "___syscall197",
        ImportValue::Func(syscalls::___syscall197 as _),
    );
    import_object.set(
        "env",
        "___syscall180",
        ImportValue::Func(syscalls::___syscall180 as _),
    );
    import_object.set(
        "env",
        "___syscall181",
        ImportValue::Func(syscalls::___syscall181 as _),
    );
    import_object.set(
        "env",
        "___syscall39",
        ImportValue::Func(syscalls::___syscall39 as _),
    );
    import_object.set(
        "env",
        "___syscall195",
        ImportValue::Func(syscalls::___syscall195 as _),
    );
    import_object.set(
        "env",
        "___syscall212",
        ImportValue::Func(syscalls::___syscall212 as _),
    );
    import_object.set(
        "env",
        "___syscall221",
        ImportValue::Func(syscalls::___syscall221 as _),
    );
    import_object.set(
        "env",
        "___syscall102",
        ImportValue::Func(syscalls::___syscall102 as _),
    );
    import_object.set(
        "env",
        "___syscall54",
        ImportValue::Func(syscalls::___syscall54 as _),
    );
    import_object.set(
        "env",
        "___syscall12",
        ImportValue::Func(syscalls::___syscall12 as _),
    );
    import_object.set(
        "env",
        "___syscall192",
        ImportValue::Func(syscalls::___syscall192 as _),
    );
    import_object.set(
        "env",
        "___syscall63",
        ImportValue::Func(syscalls::___syscall63 as _),
    );
    import_object.set(
        "env",
        "___syscall142",
        ImportValue::Func(syscalls::___syscall142 as _),
    );
    import_object.set(
        "env",
        "___syscall57",
        ImportValue::Func(syscalls::___syscall57 as _),
    );

    // Process
    import_object.set("env", "abort", ImportValue::Func(process::em_abort as _));
    import_object.set("env", "_abort", ImportValue::Func(process::_abort as _));
    import_object.set(
        "env",
        "abortStackOverflow",
        ImportValue::Func(process::abort_stack_overflow as _),
    );
    import_object.set("env", "_fork", ImportValue::Func(process::_fork as _));
    import_object.set("env", "_exit", ImportValue::Func(process::_exit as _));

    // Signal
    import_object.set(
        "env",
        "_sigemptyset",
        ImportValue::Func(signal::_sigemptyset as _),
    );
    import_object.set(
        "env",
        "_sigaddset",
        ImportValue::Func(signal::_sigaddset as _),
    );
    import_object.set(
        "env",
        "_sigprocmask",
        ImportValue::Func(signal::_sigprocmask as _),
    );
    import_object.set(
        "env",
        "_sigaction",
        ImportValue::Func(signal::_sigaction as _),
    );
    import_object.set(
        "env",
        "_signal",
        ImportValue::Func(signal::_signal as _),
    );
    // Memory
    import_object.set(
        "env",
        "abortOnCannotGrowMemory",
        ImportValue::Func(memory::abort_on_cannot_grow_memory as _),
    );
    import_object.set(
        "env",
        "_emscripten_memcpy_big",
        ImportValue::Func(memory::_emscripten_memcpy_big as _),
    );
    import_object.set(
        "env",
        "enlargeMemory",
        ImportValue::Func(memory::enlarge_memory as _),
    );
    import_object.set(
        "env",
        "getTotalMemory",
        ImportValue::Func(memory::get_total_memory as _),
    );
    // NullFuncs
    import_object.set(
        "env",
        "nullFunc_ii",
        ImportValue::Func(nullfunc::nullfunc_ii as _),
    );
    import_object.set(
        "env",
        "nullFunc_iii",
        ImportValue::Func(nullfunc::nullfunc_iii as _),
    );
    import_object.set(
        "env",
        "nullFunc_iiii",
        ImportValue::Func(nullfunc::nullfunc_iiii as _),
    );
    import_object.set(
        "env",
        "nullFunc_iiiii",
        ImportValue::Func(nullfunc::nullfunc_iiiii as _),
    );
    import_object.set(
        "env",
        "nullFunc_iiiiii",
        ImportValue::Func(nullfunc::nullfunc_iiiiii as _),
    );
    import_object.set(
        "env",
        "nullFunc_vi",
        ImportValue::Func(nullfunc::nullfunc_vi as _),
    );
    import_object.set(
        "env",
        "nullFunc_vii",
        ImportValue::Func(nullfunc::nullfunc_vii as _),
    );
    import_object.set(
        "env",
        "nullFunc_viii",
        ImportValue::Func(nullfunc::nullfunc_viii as _),
    );
    import_object.set(
        "env",
        "nullFunc_viiii",
        ImportValue::Func(nullfunc::nullfunc_viiii as _),
    );
    // Time
    import_object.set(
        "env",
        "_gettimeofday",
        ImportValue::Func(time::_gettimeofday as _),
    );
    import_object.set(
        "env",
        "_clock_gettime",
        ImportValue::Func(time::_clock_gettime as _),
    );
    import_object.set(
        "env",
        "_localtime",
        ImportValue::Func(time::_localtime as _),
    );
    import_object.set("env", "_time", ImportValue::Func(time::_time as _));
    import_object.set("env", "_strftime", ImportValue::Func(time::_strftime as _));
    import_object.set(
        "env",
        "_localtime_r",
        ImportValue::Func(env::_localtime_r as _),
    );
    import_object.set(
        "env",
        "_getpagesize",
        ImportValue::Func(env::_getpagesize as _),
    );
    import_object.set(
        "env",
        "_sysconf",
        ImportValue::Func(env::_sysconf as _),
    );

    mock_external!(import_object, _waitpid);
    mock_external!(import_object, _utimes);
    mock_external!(import_object, _usleep);
    // mock_external!(import_object, _time);
    // mock_external!(import_object, _sysconf);
    // mock_external!(import_object, _strftime);
    mock_external!(import_object, _sigsuspend);
    // mock_external!(import_object, _sigprocmask);
    // mock_external!(import_object, _sigemptyset);
    // mock_external!(import_object, _sigaddset);
    // mock_external!(import_object, _sigaction);
    mock_external!(import_object, _setitimer);
    mock_external!(import_object, _setgroups);
    mock_external!(import_object, _setgrent);
    mock_external!(import_object, _sem_wait);
    mock_external!(import_object, _sem_post);
    mock_external!(import_object, _sem_init);
    mock_external!(import_object, _sched_yield);
    mock_external!(import_object, _raise);
    mock_external!(import_object, _mktime);
    // mock_external!(import_object, _localtime_r);
    // mock_external!(import_object, _localtime);
    mock_external!(import_object, _llvm_stacksave);
    mock_external!(import_object, _llvm_stackrestore);
    mock_external!(import_object, _kill);
    mock_external!(import_object, _gmtime_r);
    // mock_external!(import_object, _gettimeofday);
    // mock_external!(import_object, _getpagesize);
    mock_external!(import_object, _getgrent);
    mock_external!(import_object, _getaddrinfo);
    // mock_external!(import_object, _fork);
    // mock_external!(import_object, _exit);
    mock_external!(import_object, _execve);
    mock_external!(import_object, _endgrent);
    // mock_external!(import_object, _clock_gettime);
    mock_external!(import_object, ___syscall97);
    mock_external!(import_object, ___syscall91);
    mock_external!(import_object, ___syscall85);
    mock_external!(import_object, ___syscall75);
    mock_external!(import_object, ___syscall66);
    // mock_external!(import_object, ___syscall64);
    // mock_external!(import_object, ___syscall63);
    mock_external!(import_object, ___syscall60);
    // mock_external!(import_object, ___syscall54);
    // mock_external!(import_object, ___syscall39);
    mock_external!(import_object, ___syscall38);
    // mock_external!(import_object, ___syscall340);
    mock_external!(import_object, ___syscall334);
    mock_external!(import_object, ___syscall300);
    mock_external!(import_object, ___syscall295);
    mock_external!(import_object, ___syscall272);
    mock_external!(import_object, ___syscall268);
    // mock_external!(import_object, ___syscall221);
    mock_external!(import_object, ___syscall220);
    // mock_external!(import_object, ___syscall212);
    // mock_external!(import_object, ___syscall201);
    mock_external!(import_object, ___syscall199);
    // mock_external!(import_object, ___syscall197);
    mock_external!(import_object, ___syscall196);
    // mock_external!(import_object, ___syscall195);
    mock_external!(import_object, ___syscall194);
    mock_external!(import_object, ___syscall191);
    // mock_external!(import_object, ___syscall181);
    // mock_external!(import_object, ___syscall180);
    mock_external!(import_object, ___syscall168);
    // mock_external!(import_object, ___syscall146);
    // mock_external!(import_object, ___syscall145);
    // mock_external!(import_object, ___syscall142);
    mock_external!(import_object, ___syscall140);
    // mock_external!(import_object, ___syscall122);
    // mock_external!(import_object, ___syscall102);
    // mock_external!(import_object, ___syscall20);
    mock_external!(import_object, ___syscall15);
    mock_external!(import_object, ___syscall10);

    import_object
}

#[cfg(test)]
mod tests {
    use super::generate_emscripten_env;
    use crate::webassembly::{instantiate, Export, Instance};

    #[test]
    fn test_putchar() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/putchar.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        let func_index = match result_object.module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn(&Instance) = get_instance_function!(result_object.instance, func_index);
        main(&result_object.instance);
    }

    #[test]
    fn test_print() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/printf.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        let func_index = match result_object.module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn(&Instance) = get_instance_function!(result_object.instance, func_index);
        main(&result_object.instance);
    }
}
