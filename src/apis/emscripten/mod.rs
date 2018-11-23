use crate::webassembly::{ImportObject, ImportValue};

// EMSCRIPTEN APIS
mod env;
mod io;
mod memory;
mod process;
mod syscalls;
mod utils;
mod varargs;

// SYSCALLS
pub use self::utils::is_emscripten_module;

pub fn generate_emscripten_env<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("env", "printf", ImportValue::Func(io::printf as *const u8));
    import_object.set(
        "env",
        "putchar",
        ImportValue::Func(io::putchar as *const u8),
    );
    // Emscripten Env
    import_object.set(
        "env",
        "_getenv",
        ImportValue::Func(env::_getenv as *const u8),
    );
    // Emscripten syscalls
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
        "___syscall54",
        ImportValue::Func(syscalls::___syscall54 as *const u8),
    );
    import_object.set(
        "env",
        "___syscall122",
        ImportValue::Func(syscalls::___syscall122 as *const u8),
    );
    // Emscripten other APIs
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
        "abortOnCannotGrowMemory",
        ImportValue::Func(process::abort_on_cannot_grow_memory as *const u8),
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
