use crate::webassembly::{ImportObject, VmCtx};
use libc::{printf, putchar};

extern "C" fn _printf(memory_offset: i32, extra: i32, vm_context: &mut VmCtx) -> i32 {
    // println!("PRINTF");
    let mem = &vm_context.user_data.instance.memories[0];
    // let x = mem.carve_slice(memory_offset as u32, 16).unwrap();
    return unsafe {
        let base_memory_offset = mem.mmap.as_ptr().offset(memory_offset as isize) as *const i8;
        printf(base_memory_offset, extra)
    };
}

pub fn generate_libc_env<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("env", "printf", _printf as *const u8);
    import_object.set("env", "putchar", putchar as *const u8);
    import_object
}

#[cfg(test)]
mod tests {
    use super::generate_libc_env;
    use crate::webassembly::{instantiate, Export, VmCtx};

    #[test]
    fn test_putchar() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/putchar.wast");
        let import_object = generate_libc_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        let module = result_object.module;
        let instance = result_object.instance;
        let func_index = match module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn(&VmCtx) = get_instance_function!(instance, func_index);
        let context = instance.generate_context();
        main(&context);
    }

    #[test]
    fn test_print() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/printf.wast");
        let import_object = generate_libc_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        let module = result_object.module;
        let instance = result_object.instance;
        let func_index = match module.info.exports.get("main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let main: fn(&VmCtx) = get_instance_function!(instance, func_index);
        let context = instance.generate_context();
        main(&context);
    }
}
