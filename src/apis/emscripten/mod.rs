/// NOTE: TODO: These emscripten api implementation only support wasm32 for now because they assume offsets are u32
<<<<<<< HEAD
use crate::webassembly::{ImportObject, ImportValue, LinearMemory};
use byteorder::{ByteOrder, LittleEndian};
use std::mem;
=======
use crate::webassembly::{ImportObject, ImportValue};
>>>>>>> Add some syscalls

// EMSCRIPTEN APIS
mod env;
mod io;
mod memory;
mod process;
mod syscalls;
mod lock;
mod utils;
mod varargs;
mod errno;
mod storage;
mod nullfunc;

pub use self::utils::is_emscripten_module;
pub use self::storage::{align_memory, static_alloc};

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
    let mem = &mut memory[dynamictop_ptr..dynamictop_ptr+mem::size_of::<u32>()];
    LittleEndian::write_u32(mem, dynamic_base(STATIC_BUMP));
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
    import_object.set(
        "env",
        "tableBase",
        ImportValue::Global(0),
    );

    // Print functions
    import_object.set("env", "printf", ImportValue::Func(io::printf as *const u8));
    import_object.set(
        "env",
        "putchar",
        ImportValue::Func(io::putchar as *const u8),
    );
    // Lock
    import_object.set(
        "env",
        "___lock",
        ImportValue::Func(lock::___lock as *const u8),
    );
    import_object.set(
        "env",
        "___unlock",
        ImportValue::Func(lock::___unlock as *const u8),
    );
    // Env
    import_object.set(
        "env",
        "_getenv",
        ImportValue::Func(env::_getenv as *const u8),
    );
    // Errno
    import_object.set(
        "env",
        "___setErrNo",
        ImportValue::Func(errno::___seterrno as *const u8),
    );
    // Syscalls
    import_object.set(
        "env",
        "___syscall1",
        ImportValue::Func(syscalls::___syscall1 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall3",
        ImportValue::Func(syscalls::___syscall3 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall4",
        ImportValue::Func(syscalls::___syscall4 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall5",
        ImportValue::Func(syscalls::___syscall5 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall6",
        ImportValue::Func(syscalls::___syscall6 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall54",
        ImportValue::Func(syscalls::___syscall54 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall140",
        ImportValue::Func(syscalls::___syscall140 as *const u8),
<<<<<<< HEAD
    );
    import_object.set(
        "env",
        "___syscall145",
        ImportValue::Func(syscalls::___syscall145 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall146",
        ImportValue::Func(syscalls::___syscall146 as *const u8),
    );
    import_object.set(
        "env",
=======
    );
    import_object.set(
        "env",
        "___syscall145",
        ImportValue::Func(syscalls::___syscall145 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall146",
        ImportValue::Func(syscalls::___syscall146 as *const u8),
    );
    import_object.set(
        "env",
>>>>>>> Add some syscalls
        "___syscall221",
        ImportValue::Func(syscalls::___syscall221 as *const u8),
    );
    // Process
    import_object.set(
        "env",
        "abort",
        ImportValue::Func(process::em_abort as *const u8),
    );
    import_object.set(
        "env",
        "_abort",
        ImportValue::Func(process::_abort as *const u8),
    );
    import_object.set(
        "env",
        "abortStackOverflow",
        ImportValue::Func(process::abort_stack_overflow as *const u8),
    );
    // Memory
    import_object.set(
        "env",
        "abortOnCannotGrowMemory",
        ImportValue::Func(memory::abort_on_cannot_grow_memory as *const u8),
    );
    import_object.set(
        "env",
        "_emscripten_memcpy_big",
        ImportValue::Func(memory::_emscripten_memcpy_big as *const u8),
    );
    import_object.set(
        "env",
        "enlargeMemory",
        ImportValue::Func(memory::enlarge_memory as *const u8),
    );
    import_object.set(
        "env",
        "getTotalMemory",
        ImportValue::Func(memory::get_total_memory as *const u8),
    );
    // NullFuncs
    import_object.set(
        "env",
        "nullFunc_ii",
        ImportValue::Func(nullfunc::nullfunc_ii as *const u8),
    );
    import_object.set(
        "env",
<<<<<<< HEAD
        "nullFunc_iii",
        ImportValue::Func(nullfunc::nullfunc_iii as *const u8),
=======
        "nullFunc_iiii",
        ImportValue::Func(nullfunc::nullfunc_iiii as *const u8),
>>>>>>> Add some syscalls
    );
    import_object.set(
        "env",
        "nullFunc_iiii",
        ImportValue::Func(nullfunc::nullfunc_iiii as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_iiiii",
        ImportValue::Func(nullfunc::nullfunc_iiiii as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_iiiiii",
        ImportValue::Func(nullfunc::nullfunc_iiiiii as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_vi",
        ImportValue::Func(nullfunc::nullfunc_vi as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_vii",
        ImportValue::Func(nullfunc::nullfunc_vii as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_viii",
        ImportValue::Func(nullfunc::nullfunc_viii as *const u8),
    );
    import_object.set(
        "env",
        "nullFunc_viiii",
        ImportValue::Func(nullfunc::nullfunc_viiii as *const u8),
    );
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
